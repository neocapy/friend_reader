# friend reader

multiplayer epub/txt reader that you can use with your friends (and you can follow them, and see where they are...)

it's super basic for now, just an mvp. don't expect much

## how do i use it

well, first you build it

```
cargo build --release
```

someone has to have the server and the .epub or .txt file locally

```
./target/release/server <path_to_file>
```

then everyone else opens the client and puts the IP address etc in to the UI

```
./target/release/client
```