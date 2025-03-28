# HTTP Shell Executor

A simple server serving shell script. It can act like a simple script based CI/CD server.

## Usage

```bash
./http-shell-executor --help
```

```bash
./http-shell-executor
./http-shell-executor --upload
./http-shell-executor --listen 0.0.0.0:8000
```

## Serving

With the default configuration, static files are served from `./public/` and serving at `/` using GET method, and dynamic scripts are served from `./scripts/` and serving at `/` with POST method.

Static files are serving with `Content-Type` header which is detected by file extension. Dynamic scripts' headers are always `Content-Type: text/plain; charset=utf-8` abd `X-Content-Type-Options: nosniff`.

Streaming response is supported for scripts. Script response is always trunked. It would be continued until the script exits or the connection is closed. If the script exits, the response will stop and the connection will be closed. If the connection is closed, the script will receive a `SIGTERM` signal.

Generated homepage not support. Please use static homepage + Fetch API.

Upload is supported. You need to add `--upload` in command line. You can PUT file to `./public/uploads/` dir via `/uploads/` path. The file will be overwritten if it exists.

## Scripting

Script must start with [`#!`](https://en.wikipedia.org/wiki/Shebang_(Unix)). You can also use a binary executable. Scripts and executables need to be `chmod +x`.

script arguments are passed as query string and `application/x-www-form-urlencoded` body. every `args=value` and `args[]=value` will be considered as an argument.

You may love [jq](https://github.com/stedolan/jq) for script programming.

## Examples

examples can be found in `public/` and `scripts/` dir.

## Build

Build `./target/x86_64-unknown-linux-musl/release/http-shell-executor` with [muslrust](https://hub.docker.com/r/clux/muslrust/).

```bash
docker run -it --rm -v cargo-cache:/root/.cargo/registry -v "$(pwd)":/volume clux/muslrust:stable cargo build --release
```

## License

[MIT License](https://mit-license.org/license.txt)
