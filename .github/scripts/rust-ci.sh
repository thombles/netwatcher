set -eu

cargo build
cargo test --lib -- --nocapture
cargo test --test api_test -- --include-ignored --nocapture
cargo test --features tokio --test async_api_test -- --include-ignored --nocapture
cargo test --features async-io --test async_api_test -- --include-ignored --nocapture
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features tokio -- -D warnings
cargo clippy --all-targets --features async-io -- -D warnings
cargo fmt --all -- --check
