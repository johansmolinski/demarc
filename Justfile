

install:
    cargo build --release
    sudo cp target/release/demarc /usr/local/bin

test:
    cargo test
