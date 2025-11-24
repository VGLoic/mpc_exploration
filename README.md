# MPC Exploration

This repository is an exploration of multi-party computation (MPC) techniques and their applications. The goal is to experiment with different MPC protocols, libraries, and use cases to better understand their capabilities and limitations.

## Current features

### Addition protocol

A simple addition protocol has been implemented using Shamir secret sharing. This protocol allows multiple parties to securely compute the sum of their private inputs without revealing them to each other.

Once a process is created, each server peer will poll the other peers for completion of the process.

The protocol flows as follows:
1. magical user generates a new process ID,
2. magical user creates a new addition process by sending a request to all peers with the new process ID. Peer servers will create the process with a random input,
3. each peer server will periodically poll the other peers to retrieve their missing input shares,
4. once all shares are collected, each peer server will reconstruct the sum and store the result,
5. each peer server will periodically poll the other peers to retrieve their missing shares sums,
6. once all shares sums are collected, each peer server will reconstruct the final sum result.

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


### Running the addition protocol

Creation of a new addition process can be done by running the `new_addition` binary with the required parameters:

```bash
cargo run --bin new_addition -- ports=<port1,port2,...>
```

Where `ports` is a comma-separated list of peer server ports. Localhost is assumed for all peer servers.

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
