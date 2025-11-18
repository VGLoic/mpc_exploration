# MPC Exploration

This repository is an exploration of multi-party computation (MPC) techniques and their applications. The goal is to experiment with different MPC protocols, libraries, and use cases to better understand their capabilities and limitations.

## Current features

### Addition protocol

A simple addition protocol has been implemented using Shamir secret sharing. This protocol allows multiple parties to securely compute the sum of their private inputs without revealing them to each other.

A process is initiated by a server peer, it generates a random input and shares it with other peers.

Reception of a share from a peer triggers the generation of a new random input, which is then shared with all other peers.

This protocol assumes for now that all peers are honest and follow the protocol correctly.

See the associated [integration test](./tests/addition_test.rs) for a running example.

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

### Unit tests

Unit tests can be run:
```bash
cargo test --lib
```

### Integration tests

Integration tests can be run:
```bash
cargo test --tests
```
