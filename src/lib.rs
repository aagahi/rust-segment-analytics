extern crate time;

use std::thread;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::fmt::Debug;
use std::time::Duration;
use std::fmt::Display;
use std::collections::HashMap;

#[macro_use]
extern crate hyper;
use hyper::Client;
use hyper::header::{Headers, Authorization, Basic, ContentType};




// ------------------------------------------------------------------------------------------------
// Worker API
// ------------------------------------------------------------------------------------------------

struct ThreadState<'a> {
    alive: &'a mut Arc<AtomicBool>,
}
impl<'a> ThreadState<'a> {
    fn set_alive(&self) {
        self.alive.store(true, Ordering::Relaxed);
    }
}
impl<'a> Drop for ThreadState<'a> {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
    }
}

pub trait WorkerClosure<T, P>: Fn(&P, T) -> () + Send + Sync {}
impl<T, F, P> WorkerClosure<T, P> for F where F: Fn(&P, T) -> () + Send + Sync {}


pub struct SingleWorker<T: 'static + Send, P: Clone + Send> {
    parameters: P,
    f: Arc<Box<WorkerClosure<T, P, Output = ()>>>,
    receiver: Arc<Mutex<Receiver<T>>>,
    sender: Mutex<Sender<T>>,
    alive: Arc<AtomicBool>,
}

impl<T: 'static + Debug + Send, P: 'static + Clone + Send> SingleWorker<T, P> {
    pub fn new(parameters: P, f: Box<WorkerClosure<T, P, Output = ()>>) -> SingleWorker<T, P> {
        let (sender, receiver) = channel::<T>();

        let worker = SingleWorker {
            parameters: parameters,
            f: Arc::new(f),
            receiver: Arc::new(Mutex::new(receiver)),
            sender: Mutex::new(sender), /* too bad sender is not sync -- suboptimal.... see https://github.com/rust-lang/rfcs/pull/1299/files */
            alive: Arc::new(AtomicBool::new(true)),
        };
        SingleWorker::spawn_thread(&worker);
        worker
    }

    fn is_alive(&self) -> bool {
        self.alive.clone().load(Ordering::Relaxed)
    }

    fn spawn_thread(worker: &SingleWorker<T, P>) {
        let mut alive = worker.alive.clone();
        let f = worker.f.clone();
        let receiver = worker.receiver.clone();
        let parameters = worker.parameters.clone();
        thread::spawn(move || {
            let state = ThreadState { alive: &mut alive };
            state.set_alive();

            let lock = match receiver.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            loop {
                match lock.recv() {
                    Ok(value) => f(&parameters, value),
                    Err(_) => {
                        thread::yield_now();
                    }
                };
            }

        });
        while !worker.is_alive() {
            thread::yield_now();
        }
    }

    pub fn work_with(&self, msg: T) {
        let alive = self.is_alive();
        if !alive {
            SingleWorker::spawn_thread(self);
        }

        let lock = match self.sender.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let _ = lock.send(msg);
    }
}

// ------------------------------------------------------------------------------------------------
// Segment API
// ------------------------------------------------------------------------------------------------

#[derive(Debug,Clone)]
pub struct SegmentQuery {
    url: String,
    body: String,
}
pub struct Segment {
    worker: Arc<SingleWorker<SegmentQuery, Option<String>>>,
}
pub trait ToJsonString {
    fn to_json_string(&self) -> String;
}

impl ToJsonString for String {
    fn to_json_string(&self) -> String {
        self.to_owned()
    }
}


impl<V> ToJsonString for HashMap<&'static str, V>
    where V: Display
{
    fn to_json_string(&self) -> String {
        let mut jstr = String::new();
        jstr.push_str("{");
        let mut passed = false;
        for (k, v) in self {
            if passed {
                jstr.push_str(",");
            } else {
                passed = true;
            }
            jstr.push_str(&format!("\"{}\":\"{}\"", k, v));
        }
        jstr.push_str("}");
        jstr
    }
}

impl Segment {
    pub fn new(write_key: Option<String>) -> Segment {
        let worker = SingleWorker::new(write_key,
                                       Box::new(move |write_key, query| -> () {
                                           Segment::post(write_key, &query);
                                       }));
        Segment { worker: Arc::new(worker) }
    }

    fn post(write_key: &Option<String>, query: &SegmentQuery) {
        if let Some(key) = write_key.clone() {

            let mut headers = Headers::new();
            headers.set(Authorization(Basic {
                username: key.clone(),
                password: None,
            }));
            headers.set(ContentType::json());

            let mut client = Client::new();
            client.set_read_timeout(Some(Duration::new(5, 0)));
            client.set_write_timeout(Some(Duration::new(5, 0)));

            match client.post(&query.url)
                .headers(headers)
                .body(&query.body)
                .send() {
                Ok(response) => {
                    if response.status != hyper::Ok {
                        println!("ERROR: Segment service returned error code {} for query {:?}",
                                 response.status,
                                 query);
                    }
                }
                Err(err) => {
                    println!("ERROR: fail to post segment query {:?} - Error {}",
                             query,
                             err);
                }

            }



        }
    }



    pub fn alias(&self, previous_id: &str, user_id: &str) {
        let mut body = String::new();
        body.push_str("{");
        body.push_str(&format!("\"previousId\":\"{}\",", previous_id));
        body.push_str(&format!("\"userId\":\"{}\"", user_id));
        body.push_str("}");
        self.worker.work_with(SegmentQuery {
            url: "https://api.segment.io/v1/alias".to_string(),
            body: body,
        });
    }


    pub fn identify<T1: ToJsonString, T2: ToJsonString>(&self,
                                                        anonymous_id: Option<&str>,
                                                        user_id: Option<&str>,
                                                        traits: Option<T1>,
                                                        context: Option<T2>) {


        let mut body = String::new();
        body.push_str("{");
        if let Some(anonymous_id) = anonymous_id {
            body.push_str(&format!("\"anonymousId\":\"{}\"", anonymous_id));
        }
        if let Some(user_id) = user_id {
            if body.len() > 1 {
                body.push_str(",")
            }
            body.push_str(&format!("\"userId\":\"{}\"", user_id));
        }
        if let Some(traits) = traits {
            if body.len() > 1 {
                body.push_str(",")
            }
            body.push_str(&format!("\"traits\":{}", traits.to_json_string()));
        }
        if let Some(context) = context {
            if body.len() > 1 {
                body.push_str(",")
            }
            body.push_str(&format!("\"context\":{}", context.to_json_string()));
        }

        body.push_str("}");

        self.worker.work_with(SegmentQuery {
            url: "https://api.segment.io/v1/identify".to_string(),
            body: body,
        });
    }



    pub fn track<T1: ToJsonString, T2: ToJsonString>(&self,
                                                     anonymous_id: Option<&str>,
                                                     user_id: Option<&str>,
                                                     event: &str,
                                                     properties: Option<T1>,
                                                     context: Option<T2>) {

        let mut body = String::new();
        body.push_str("{");
        body.push_str(&format!("\"event\":\"{}\",", event));

        if let Some(anonymous_id) = anonymous_id {
            body.push_str(&format!(",\"anonymousId\":\"{}\"", anonymous_id));
        }
        if let Some(user_id) = user_id {
            body.push_str(&format!(",\"userId\":\"{}\",", user_id));
        }
        if let Some(properties) = properties {
            body.push_str(&format!(",\"properties\":{}", properties.to_json_string()));
        }
        if let Some(context) = context {
            body.push_str(&format!(",\"context\":{}", context.to_json_string()));
        }
        body.push_str("}");

        self.worker.work_with(SegmentQuery {
            url: "https://api.segment.io/v1/track".to_string(),
            body: body,
        });

    }
}


#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::thread;
    use std::time::Duration;
    use std::sync::Arc;

    // Segment Test Key - do not abuse of it ;)
    static SEGMENT_WRITE_KEY: &'static str = "okSiXGEgvMbIOjlmQFDq034TJIfnomu6";


    #[test]
    fn it_should_send_alias_message() {
        let segment = ::Segment::new(Some(SEGMENT_WRITE_KEY.to_string()));
        segment.alias("previous_id", "user_id");

        // yeah I know ;)
        thread::sleep(Duration::new(5, 0));
    }


    #[test]
    fn it_should_send_identify_message() {
        let segment = ::Segment::new(Some(SEGMENT_WRITE_KEY.to_string()));
        let mut context = HashMap::new();
        context.insert("ip", "134.157.15.3");
        segment.identify(Some("anonymous_id"), None, None::<String>, Some(context));

        // yeah I know ;)
        thread::sleep(Duration::new(5, 0));
    }

    #[test]
    fn it_should_send_track_message() {
        let segment = ::Segment::new(Some(SEGMENT_WRITE_KEY.to_string()));
        let mut properties = HashMap::new();
        properties.insert("firstname", "Jimmy");
        properties.insert("lastname", "Page");

        segment.track(Some("anonymous_id"),
                      None,
                      "Test Event",
                      Some(properties),
                      None::<String>);

        // yeah I know ;)
        thread::sleep(Duration::new(5, 0));

    }


    #[test]
    fn it_should_send_many_message() {
        let segment = Arc::new(::Segment::new(Some(SEGMENT_WRITE_KEY.to_string())));

        let segment1 = segment.clone();
        let t1 = thread::spawn(move || {
            segment1.track(Some("anonymous_id"),
                           None,
                           "Test Event 1",
                           None::<String>,
                           None::<String>)
        });


        let segment2 = segment.clone();
        let t2 = thread::spawn(move || {
            segment2.track(Some("anonymous_id"),
                           None,
                           "Test Event 2",
                           None::<String>,
                           None::<String>)
        });

        let _ = t1.join().unwrap();
        let _ = t2.join().unwrap();

        // yeah I know ;)
        thread::sleep(Duration::new(5, 0));

    }

}
