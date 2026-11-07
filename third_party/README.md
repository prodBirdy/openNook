# third_party

`vt100` is a patched 0.15.2 (`Grid::visible_rows` overflow). Wired via
`[patch.crates-io]` in the workspace `Cargo.toml`.

`mediaremote-adapter` is cloned and built here by
`scripts/build-mediaremote-adapter.sh` (also invoked from `scripts/bundle.sh`).

It is **not** linked into openNook. The app invokes:

```
/usr/bin/perl mediaremote-adapter.pl MediaRemoteAdapter.framework COMMAND
```

so MediaRemote runs under Perl’s `com.apple.perl` bundle ID, which still works
on macOS 15.4+. Source: https://github.com/ungive/mediaremote-adapter
