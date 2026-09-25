# App menu example

This package demonstrates the same menu of applications and figures on native
and web targets.

Run it natively from the Myplotlib repository root:

```sh
cargo run --example app_menu
```

Build and serve the web version:

```sh
cargo myplotlib build-web \
  --manifest-path examples/app_menu/Cargo.toml \
  --out-dir examples/app_menu/dist \
  --release \
  --locked
cargo myplotlib serve --dir examples/app_menu
```

Then open <http://127.0.0.1:8080/>.
