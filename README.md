# diesel_migrations_async

## DEPRECATED

Use the regular [`diesel_migrations`](https://github.com/diesel-rs/diesel/tree/master/diesel_migrations) together with [`AsyncConnectionWrapper`](https://docs.rs/diesel-async/0.4.1/diesel_async/async_connection_wrapper/type.AsyncConnectionWrapper.html) inside a `spawn_blocking` thread.

Fork of the [`diesel_migrations`](https://github.com/diesel-rs/diesel/tree/master/diesel_migrations) crate with support for the [`diesel-async`](https://github.com/weiznich/diesel_async) crate.  
Mainly intended for migrations without having to install `libpq`.

The old version history unfortunately couldn't be preserved because this fork is extracted out of the monorepo.

## Credits

The [Diesel team](https://github.com/diesel-rs) for the original implementation

## License

Licensed under either of these:

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or
   https://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or
   https://opensource.org/licenses/MIT)

### Contributing
Before contributing, please read the [contributors guide](https://github.com/diesel-rs/diesel/blob/master/CONTRIBUTING.md)
for useful information about setting up Diesel locally, coding style and common abbreviations.

Unless you explicitly state otherwise, any contribution you intentionally submit
for inclusion in the work, as defined in the Apache-2.0 license, shall be
dual-licensed as above, without any additional terms or conditions.
