

install:
    cargo build --release
    sudo cp target/release/demarc /usr/local/bin

test:
    cargo test

HOME := x'${HOME}'
ZOLA := HOME / "projects/docs/minnberg"

win:
    cargo xwin build --release --target x86_64-pc-windows-msvc
    cp target/x86_64-pc-windows-msvc/release/demarc.exe {{ZOLA}}/static/dl/

site:
    cp demarc.md {{ZOLA}}/content/
    zola -r {{ZOLA}} build
    rsync -avz {{ZOLA}}/public/ sasq@minnberg.se:/var/www/html/
