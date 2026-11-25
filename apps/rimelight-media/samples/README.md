# Samples

One small file per type Rimelight Media opens, so each can be tried in the app.

- Text and SVG files are here already, written by hand (`sample.txt`, `sample.json`, `sample.env`, `sample.log`,
  `sample.svg` and so on).
- Pictures, sound and video are made by `make-samples.ps1`, which needs FFmpeg (1.5 s clips, 64 x 64 pictures):

      powershell -ExecutionPolicy Bypass -File samples\make-samples.ps1

  It finds `ffmpeg.exe` on the PATH or under `ffmpeg\` and `tools\`, or take `-Ffmpeg <path>`. A format whose encoder
  the FFmpeg build lacks is skipped with a warning.

## Not generated

These can't be made from scratch. Real files are on [raw.pixls.us](https://raw.pixls.us) (sample RAW files for most
camera makers), and any phone's HEIC photo will do.

- Camera RAW: `.3fr`, `.arw`, `.cr2`, `.cr3`, `.dng`, `.erf`, `.iiq`, `.kdc`, `.mrw`, `.nef`, `.nrw`, `.orf`, `.pef`,
  `.raf`, `.rw2`, `.rwl`, `.sr2`, `.srf`, `.srw`, `.x3f`
- HEIC / HEIF: `.heic`, `.heif`, `.hif`
