# HTTP Shell Executor

A simple server serving shell script. You can use it as a simple script based CI/CD server.

## Usage

```bash
./http-shell-executor --help
```

```bash
./http-shell-executor --listen 0.0.0.0:8000
```

## Serving

With the default configuration, static files are served from `./public/` and serving at `/` using GET method, and dynamic scripts are served from `./scripts/` and serving at `/` with POST method.

Static files are serving with `Content-Type` header which is detected by file extension. Dynamic scripts' headers are always `Content-Type: text/event-stream; charset=utf-8`.

Streaming support. If your script not finished, it would convert to a streaming mode.

Generated homepage not support. Please use static homepage + Fetch API.

## Scripting

Script must start with [shebang](https://en.wikipedia.org/wiki/Shebang_(Unix)). You can also use an binary executable.

## Example

example can be found in `public/` and `scripts/` dir.

### Run

```bash
./http-shell-executor
```

You may like [jq](https://github.com/stedolan/jq).
