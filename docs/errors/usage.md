# `usage`

The command line or a file it names cannot be used.

An unknown option or command, a missing argument, an unreadable file or an
`include` that names a file that does not exist. `zin help` lists the
commands and options. For a missing included file, the error suggests the
closest `.zin` file next to it.

## Example

```console
$ zin check --format yaml account.zin
```

```text
unknown format 'yaml'
```

## Fix

Use one of the documented values.

```console
$ zin check --format json account.zin
```
