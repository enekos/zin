# Zin for Zed

Tree-sitter syntax highlighting for `.zin` specifications, including bracket
matching, comment toggling, indentation, and an outline of systems, actions,
and scenarios.

## Install as a development extension

1. Clone `https://github.com/enekos/zin`.
2. Run `zed: install dev extension` from Zed's command palette.
3. Select this `editors/zed` directory.
4. Open a `.zin` file. Zed detects the suffix automatically.

The extension manifest pins the grammar to a Git revision in the zin
repository. Zed downloads and compiles that revision when installing the
extension.

## Develop the grammar

```sh
cd editors/zed/tree-sitter
npm ci
npm run generate
npm test
npm run test:examples
```

Commit grammar changes first, then update `[grammars.zin].rev` in
`extension.toml` to that commit's full SHA. The corpus tests cover the
language's declarations, expressions, actions, and scenarios; the example test
parses every tracked `.zin` file and fails on any syntax error.
