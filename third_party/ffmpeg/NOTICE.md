# FFmpeg in Media Identifier

Media Identifier includes two programs from the FFmpeg project, `ffmpeg` and `ffprobe`. The app
runs them as separate programs to read media files and decode their audio; they are not linked
into the app. FFmpeg is licensed under the GNU Lesser General Public License, version 2.1 or later
(LGPL-2.1-or-later); the licence text is in `COPYING.LGPLv2.1` next to this file. FFmpeg is a
trademark of Fabrice Bellard.

## Source

- Version: 9.0.2
- Source: https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz
- SHA-256: 8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e
- Signature: the pinned tarball's `.asc` signature verifies with the FFmpeg release signing key
  `FCF986EA15E6E293A5644F10B4322F04D67658D8`; the build script then accepts only a tarball with
  this SHA-256.

The same source tarball is attached to every Media Identifier release on GitHub, next to the
installers. The source is unmodified. The build script is `scripts/build-ffmpeg.sh` in the
Media Identifier repository.

## Configure options

Both platforms:

```text
--disable-everything
--disable-autodetect
--disable-doc
--disable-debug
--disable-network
--disable-ffplay
--disable-avdevice
--disable-swscale
--enable-ffmpeg
--enable-ffprobe
--enable-static
--disable-shared
--enable-protocol=file,pipe
--enable-demuxer=matroska,mov,avi,mpegps,mpegts,mpegvideo
--enable-decoder=aac,aac_latm,ac3,eac3,mp2,mp2float,mp3,mp3float,dca,truehd,mlp,flac,opus,vorbis,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,pcm_s32le,pcm_f32le,pcm_u8,pcm_dvd,pcm_bluray,pcm_alaw,pcm_mulaw,subrip,ass,ssa,webvtt,movtext,text
--enable-encoder=pcm_f32le,pcm_s16le,subrip,srt
--enable-muxer=pcm_f32le,pcm_s16le,wav,srt,null
--enable-parser=aac,aac_latm,ac3,dca,flac,mlp,mpegaudio,opus,vorbis,h264,hevc,mpegvideo,vc1
--enable-filter=aresample,aformat,anull,atrim,format,null
```

macOS (Apple Silicon), built with the system clang:

```text
--arch=aarch64
--cc=clang
--enable-pthreads
--extra-cflags=-mmacosx-version-min=11.0
--extra-ldflags=-mmacosx-version-min=11.0
```

Windows (64-bit), built in an MSYS2 MINGW64 shell with gcc:

```text
--arch=x86_64
--target-os=mingw32
--enable-w32threads
--extra-ldflags=-static
```

No option enables GPL or non-free components, and `--disable-autodetect` keeps libraries
installed on the build computer out of the programs.

## Replacing the programs

You may replace the included programs with your own build of FFmpeg, for example one built from
modified source. They are next to the app's executable: inside
`Media Identifier.app/Contents/MacOS/` on macOS, and in the installation folder on Windows
(`ffmpeg.exe` and `ffprobe.exe`).
