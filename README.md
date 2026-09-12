# forkseer

`forkseer` is a small command-line tool that checks whether a specific file or directory exists in the forks of a GitHub repository.

It checks:

- Each fork's default branch.
- Any fork branches whose names do not exist in the source repository.

The path must be a single repository-root-relative path. `forkseer` does not use glob patterns, regular expressions, or recursive file searching.

## Motivation

Sometimes I want to contribute to a project that does not have an existing flake.nix to define development shell, and I'm too lazy to write one myself, so I vibed this tool to search existing flake.nix in forks for me.

## Requirements

- Rust and Cargo
- A GitHub token is recommended, especially when searching repositories with many forks

## Build

```sh
cargo build --release
```

## Usage

```sh
forkseer <OWNER/REPOSITORY> <PATH>
```

or via nix

```sh
nix run github:serephus/forkseer -- <OWNER/REPOSITORY> <PATH>
```

For example:

```sh
forkseer rust-lang/rust flake.nix
```

The path is relative to the repository root and must not start with `/`.

## Authentication

Provide a GitHub token with `--token`:

```sh
forkseer --token "$GITHUB_TOKEN" rust-lang/rust src/bootstrap/config.rs
```

A token gives GitHub API requests a higher rate limit. The token should have permission to read the repositories being searched.

## Options

- `--limit <N>`: inspect at most `N` forks.
- `--concurrency <N>`: scan this many repositories concurrently. The default is `8`.
- `--rate-limit <N>`: limit GitHub API requests per second across the whole process. The default is `4`.
- `--json`: print machine-readable JSON output.
- `-v`, `--verbose`: print scan progress and diagnostic information to stderr.
- `--token <TOKEN>`: use a GitHub token. This also reads `GITHUB_TOKEN`.
- `--include-root`: include the source repository in the search.

Use `--help` to see the complete CLI help:

```sh
forkseer --help
```

## Output

Text output includes the matching repository, branch, GitHub URL, and path:

```text
owner/example [main]
  https://github.com/owner/example
  - src/example.rs
```

Use `--verbose` to print progress, branch checks, and a final summary to stderr. This also works with `--json` without corrupting the JSON on stdout.

Use `--json` for output suitable for scripts:

```json
{
  "matches": [
    {
      "full_name": "owner/example",
      "branch": "main",
      "html_url": "https://github.com/owner/example",
      "path": "src/example.rs"
    }
  ],
  "failures": []
}
```

A missing path is not an error. Errors for individual repositories are reported under `failures`; the command exits unsuccessfully if any repository scan fails.

## License

See [LICENSE](LICENSE).
