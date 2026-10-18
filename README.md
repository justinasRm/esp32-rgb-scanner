# BLE packet demo

Flash the two boards separately; choose the correct USB port when prompted:

```sh
cargo run --release --bin sender
cargo run --release --bin receiver
```

Close each serial monitor with `Ctrl+]`. Keep both boards connected by USB,
find their port names, and start the Rust dashboard:

```sh
ls /dev/cu.*
cd dashboard
cargo run --release -- /dev/cu.SENDER_PORT /dev/cu.RECEIVER_PORT
```

Open [http://localhost:8000](http://localhost:8000). The sender advertises a
connectable GATT service with a `hello` characteristic. The receiver connects to
the sender and reads `hello`.
