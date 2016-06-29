Rust Segment Analytics Client
==========

[![Build Status](https://travis-ci.org/aagahi/rust-segment-analytics.svg?branch=master)](https://travis-ci.org/aagahi/rust-segment-analytics)
[![Crates.io](https://img.shields.io/crates/v/segment_analytics.svg?style=flat)](https://crates.io/crates/segment_analytics)
[![Coverage Status](https://coveralls.io/repos/github/aagahi/rust-segment-analytics/badge.svg?branch=master)](https://coveralls.io/github/aagahi/rust-segment-analytics?branch=master)


[Segment Analytics Client](https://www.segment.com/) now available for rust ;)


## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
segment_analytics = "0.1.2"
```

and this to your crate root:

```rust
extern crate segment_analytics;
```

## Examples

Usage with shared instance across thread.

```rust
extern crate segment_analytics;
use segment_analytics::Segment;

let segment = Arc::new(Segment::new(Some(SEGMENT_WRITE_KEY.to_string())));

let segment1 = segment.clone();
thread::spawn(move || {

  let mut properties = HashMap::new();
  properties.insert("firstname", "Jimmy");
  properties.insert("lastname", "Page");

  let mut context = HashMap::new();
  context.insert("ip", "134.157.15.3");

  segment1.track(Some("anonymous_id"),
                 None,
                 "EventName",
                 Some(properties),
                 Some(context))

  segment1.alias("anonymous_id", "user_id");

  let mut traits = HashMap::new();
  traits.insert("email", "bill@gates.com");
  let mut context = HashMap::new();
  context.insert("ip", "134.157.15.3");
  segment1.identify(None,Some("user_id"), None, Some(traits), Some(context));
});
```

Under the hood, one thread worker is in charge of sending the messages to segment endpoint. If the thread drops, a new one is created.


Traits, Properties and Context must implement `ToJsonString` (I'm not a fan of current json solutions in rust).

```rust
pub trait ToJsonString {
    fn to_json_string(&self) -> String;
}
```

For convenience `ToJsonString` is (basically) implemented for `HashMap`.



## License

Licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
