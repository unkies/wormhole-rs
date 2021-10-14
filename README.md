# wormhole-rs

A rust implementation of ipc channel.

## Example

```rust
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
struct TestMessage {
    a: i32,
    b: String,
    c: Vec<i32>,
}

// ....

let (mut sender, mut receiver) =
    channel::<TestMessage>().expect("failed to create channel");
let test_message = TestMessage {
    a: 1776,
    b: String::from("hello world!"),
    c: vec![5, 4, 3, 2, 1],
};

match unsafe { nix::unistd::fork().expect("failed fork") } {
    nix::unistd::ForkResult::Parent { child } => {
        let res = receiver
            .recv()
            .expect("failed to receive from child process");
        wait::waitpid(child, None).expect("failed wait for pid");
        assert_eq!(
            test_message, res,
            "received message doesn't match the send message"
        );
    }
    nix::unistd::ForkResult::Child => {
        sender
            .send(test_message)
            .expect("failed to send from the child process");
        std::process::exit(0);
    }
};
```
