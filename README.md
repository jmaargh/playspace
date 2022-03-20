# playspace &thinsp; [![ci.svg]][ci] [![crates.io]][crate] [![docs.rs]][docs]

[ci.svg]: https://github.com/jmaargh/playspace/workflows/CI/badge.svg
[ci]: https://github.com/jmaargh/playspace/actions
[crates.io]: https://img.shields.io/crates/v/playspace.svg
[crate]: https://crates.io/crates/playspace
[docs.rs]: https://docs.rs/playspace/badge.svg
[docs]: https://docs.rs/playspace

A Playspace is a simple pseudo-sandbox for your convenience.

Use these for your tests that need to set/forget files and environment
variables. Maybe you'll come up with more creative uses too, you're clever
people. It's a convenience library with no hard guarantees.

```rust
Playspace::scoped(|space| {
    space.set_envs([
        ("APP_SPECIFIC_OPTION", Some("some-value")), // Set a variable
        ("CARGO_MANIFEST_DIR", None), // Unset another
    ]);
    space.write_file(
        "app-config.toml",
        r#"
        [table]
        option1 = 1
        option2 = false
        "#
    ).expect("Failed to write config file");

    // Run some command that needs these resources...

}).expect("Failed to create playspace");

// Now your environment is back where we started
```

The [docs][docs] are your friend for more details on what it does and how it works. In short, provides:

 - A new, empty, temporary working directory and return to your previous one when done
 - Clean up for any files you create in that directory while in the Playspace
 - Checkpoint and restore all environment variables on entering/leaving the Playspace
 - Some basic protection against accidentally entering more than one Playspace

For a non-async codebase:

```toml
[dependencies]
playspace = "1"
```

For async codebase you probably want:

```toml
[dependencies]
playspace = { version = "1", default-features = false, features = ["async"] }
```

For mixed, leave in the default features.

---

Heavily inspired by the [`figment::Jail`](https://docs.rs/figment/latest/figment/struct.Jail.html)
struct, thanks to [Sergio Benitez](https://github.com/SergioBenitez/) and all the `Figment` contibutors.
