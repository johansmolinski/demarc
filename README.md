## demarc

An command line emulator frontend for the demoscene

*Main goal*

Make it easy to watch demos from C64 and Amiga

* Runs multiple demos in order or shuffled
* Shows demo meta data as overlay
* CRT shader for authentic look
* Can run Amiga exes or directories
* Right-Alt hotkey for disk switch etc


## BUILD

You need _rust_.

`cargo build --release`

## RUN

You need libretro libraries for the emulated system. Libraries are searched for
in `<current_dir>/libretro`, `<exec_dir>` and `/usr/lib/libretro/`

then

`cargo run -- <files>`

or

`target/release/demarc <files>`

### Windows

Libraries are in `libretro/` 

If you copy the exe to your path, also copy the DLL:s and it should work

### Linux

If you installed retroarch you may have libs available in /usr/lib/libretro

## SHORTCUTS

```
_Right Alt_ +
D = Swap disk
N = Next file
S = Change scaling
I = Toggle Info
P = Screenshot
C = Toggle CRT filter
```


