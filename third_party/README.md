# Third-party dependencies

Project-local native dependencies live here. **Large binaries are not committed to git.**

## FFmpeg (Windows)

Expected layout after setup (see [docs/ffmpeg/FFMPEG_SETUP_WINDOWS.md](../docs/ffmpeg/FFMPEG_SETUP_WINDOWS.md)):

```text
third_party/ffmpeg/
    include/          # libavcodec/avcodec.h, libavformat/avformat.h, ...
    lib/              # avcodec.lib, avformat.lib, avutil.lib, ...
    bin/              # avcodec-61.dll, avformat-61.dll, ffmpeg.exe, ffprobe.exe, ...
```

The `third_party/ffmpeg/` tree is listed in `.gitignore`. Each developer (and CI agent) reproduces it locally using the setup guide.

## Other third-party trees

None yet. Future candidates (macOS VideoToolbox experiments, shader tooling) will be documented here when added.
