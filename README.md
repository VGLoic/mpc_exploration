# MPC Exploration

This repository is an exploration of multi-party computation (MPC) techniques and their applications. The goal is to experiment with different MPC protocols, libraries, and use cases to better understand their capabilities and limitations.

## Local development

To get started with local development, you'll need to set up your environment. Follow these steps:

1. Make sure you have [Rust](https://www.rust-lang.org/tools/install) installed on your machine. Cargo version at the time of writing is 1.88.0.
    ```bash
    cargo --version
    ```

2. Set up the environment variables in a `.env` file, the required ones are indicated with the `REQUIRED` label.
    ```bash
    cp .env.example .env
    ```

3. Verify that the unit tests are running:
    ```bash
    cargo test --lib
    ```

4. Verify that the integration tests are running:
    ```bash
    cargo test --test tests
    ```

5. Run the application
    ```bash
    cargo run .
    ```

### Integration tests

Integration tests can be run:
```bash
cargo test --tests
```
