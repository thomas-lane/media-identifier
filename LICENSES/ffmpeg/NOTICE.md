# FFmpeg in Media Identifier

Media Identifier includes two programs from the FFmpeg project, `ffmpeg` and `ffprobe`, as
separate executables next to the app. They read durations, streams and chapters from video files
and decode their audio. FFmpeg is copyright the FFmpeg developers and is licensed under the GNU
Lesser General Public License, version 2.1 or later; the full license text is in
`COPYING.LGPLv2.1` beside this file. More information: <https://ffmpeg.org/legal.html>.

These programs were built from the unmodified FFmpeg 9.0.2 source release:

- Source: <https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz>
- SHA-256: `8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e`
- Signature: <https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz.asc>, made with the FFmpeg release
  signing key `FCF9 86EA 15E6 E293 A564  4F10 B432 2F04 D676 58D8`

No GPL or non-free parts of FFmpeg are enabled, and no external libraries are linked. The build is
made by `scripts/build-ffmpeg.sh` in the Media Identifier source repository with these
configure lines, followed by `make ffmpeg ffprobe` (`make ffmpeg.exe ffprobe.exe` on Windows):

macOS (Apple Silicon):

```text
./configure --disable-everything --disable-autodetect --disable-doc --disable-debug --disable-network --disable-ffplay --disable-avdevice --disable-swscale --enable-static --disable-shared --enable-ffmpeg --enable-ffprobe --enable-swresample --enable-protocol=file,pipe --enable-demuxer=matroska,mov,avi,mpegps,mpegts --enable-decoder=ac3,ac3_fixed,eac3,aac,aac_fixed,aac_latm,mp1,mp1float,mp2,mp2float,mp3,mp3float,dca,truehd,mlp,flac,opus,vorbis,alac,pcm_* --enable-decoder=subrip,ass,ssa,webvtt,movtext,text --enable-parser=aac,aac_latm,ac3,dca,flac,mlp,mpegaudio,opus,vorbis,mpegvideo,h264,hevc,vc1 --enable-filter=aresample,aformat,anull,atrim --enable-encoder=pcm_f32le,srt --enable-muxer=pcm_f32le,srt --arch=arm64 --cc=clang --enable-pthreads --extra-cflags=-mmacosx-version-min=11.0 --extra-ldflags=-mmacosx-version-min=11.0
```

Windows (x86-64, built with the mingw-w64 cross compiler):

```text
./configure --disable-everything --disable-autodetect --disable-doc --disable-debug --disable-network --disable-ffplay --disable-avdevice --disable-swscale --enable-static --disable-shared --enable-ffmpeg --enable-ffprobe --enable-swresample --enable-protocol=file,pipe --enable-demuxer=matroska,mov,avi,mpegps,mpegts --enable-decoder=ac3,ac3_fixed,eac3,aac,aac_fixed,aac_latm,mp1,mp1float,mp2,mp2float,mp3,mp3float,dca,truehd,mlp,flac,opus,vorbis,alac,pcm_* --enable-decoder=subrip,ass,ssa,webvtt,movtext,text --enable-parser=aac,aac_latm,ac3,dca,flac,mlp,mpegaudio,opus,vorbis,mpegvideo,h264,hevc,vc1 --enable-filter=aresample,aformat,anull,atrim --enable-encoder=pcm_f32le,srt --enable-muxer=pcm_f32le,srt --enable-cross-compile --target-os=mingw32 --arch=x86_64 --cross-prefix=x86_64-w64-mingw32- --enable-w32threads --disable-x86asm --extra-ldflags=-static
```

The app runs these programs as separate processes, so they can be replaced with any other build of
ffmpeg and ffprobe: set the environment variables `MI_FFMPEG` and `MI_FFPROBE` to the full paths
of the replacements before starting the app.
