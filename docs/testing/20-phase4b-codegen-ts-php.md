# 20 — Phase 4b: TypeScript and PHP snippets, Copy as cURL

> Built and compiled on your Mac over SSH (`cargo check`, `clippy`, `fmt`, unit and snapshot tests, release build). The GUI hasn't been clicked through yet; that's this checklist.

**New**
- **Two more languages in the Code view: TypeScript (fetch) and PHP (cURL)**, which completes FR-CG-01's list — cURL, JavaScript, TypeScript, Python, Go, PHP (plus Rust, which we already had). The picker shows all seven; the values-or-`{{placeholders}}` toggle and the Copy button work the same for them.
  - TypeScript is genuinely typed, not the JS snippet with a new label: `RequestInit` for the options, typed headers, `const res: Response`, and no `any`. It imports `{ openAsBlob } from "node:fs"` (the named form, so it type-checks under a bare `tsc --strict`).
  - PHP uses `curl_init` / `curl_setopt_array` / `curl_exec`, `CURLOPT_RETURNTRANSFER`, headers as a `['Name: value']` list, `CURLFile` for file parts. A HEAD request also gets `CURLOPT_NOBODY => true`, without which the generated snippet would hang waiting for a body curl never receives.
- **Copy cURL** button in the top bar, next to Save (FR-IE-08): the active request as a cURL command in one click, without going through the Code view. It respects the Code view's variables toggle, so if you've set it to `{{kept}}` the copied command is shareable, and it warns in the status line when the command contains resolved secret values.
- **Duplicate header and form-part names are no longer silently dropped.** `fetch` takes an array of pairs and `requests` takes a list of tuples, so JavaScript, TypeScript and Python now emit every duplicate instead of letting the last one win — the ordinary single-value snippet is unchanged. Where a language genuinely cannot express it (a Python headers mapping, a PHP `CURLOPT_POSTFIELDS` array), the snippet carries a generated `NOTE:` comment naming the affected header or part names rather than quietly lying.
- **113 reviewed snapshot tests** (16 request shapes × 7 languages, plus a large-body check) pin the exact output of every generator, so an escaping regression fails the build instead of reaching a snippet you paste into a terminal.

**Not yet:** more languages (C#, Java, HTTPie, PowerShell, Ruby), syntax highlighting in the Code view, Windows `cmd`-style cURL (`^` escapes), `-d @file`, per-request timeout/redirect settings in the generated snippets, copy-as-cURL straight from a sidebar item without opening it.

## Known limits, each pinned by a test
- **cURL cannot carry a NUL byte** in an argument — a body containing `\0` is byte-exact in the other six languages and verbatim (so, lossy) in cURL. POSIX `argv` has no way to express it; the test records the behaviour rather than papering over it.
- **DEL (`0x7f`) and the C1 controls (`0x80`–`0x9f`)** are left raw in the Python, JavaScript, TypeScript and Go string literals. That is legal and byte-exact in all four, but invisible when you read the snippet; Rust (`\u{7f}`) and PHP (`\x7f`) escape them, so those two disagree cosmetically with the other four on the same input.
- `duplicates` compares header names by exact equality, so `X-A` and `x-a` are kept as two entries — an object literal keys on exact equality too, and HTTP case-insensitivity is the server's business.

## Setup
```
cd ~/inCoding/nextgen/api-tester
cargo test                      # expect 149 passed (52 core + 9 http + 18 store + 11 history + 13 interop + 36 codegen + 10 script)
cargo test -p at-app            # expect 12 passed
API_TESTER_DATA_DIR=/tmp/at-test13 cargo run --release -p at-app
```
To check a generated snippet really runs, `python3 -`, `node -`, `php -r`, `go run`, `tsc` as available.

## Checklist
| # | Do | Expect |
|---|---|---|
| R1 | POST `https://httpbin.org/post`, JSON body, a header, Basic auth → **Code** | seven language buttons: cURL, Python, JavaScript, TypeScript, Go, Rust, PHP |
| R2 | **TypeScript** | typed snippet: `const headers: Record<string, string>`, `const options: RequestInit`, `Response`; no `any` |
| R3 | **PHP** | `curl_init`, `curl_setopt_array`, `CURLOPT_RETURNTRANSFER => true`, `CURLOPT_HTTPHEADER => ['...']`, `CURLOPT_POSTFIELDS` |
| R4 | Copy the PHP snippet, run it (`php snippet.php`) | the same response you got in the app |
| R5 | Copy the TypeScript snippet, run it through `tsc --strict` (or `npx tsx`) | compiles with no errors; running it returns the same response |
| R6 | Body with `'`, `"`, a backtick, `$`, `{{notavar}}`, a newline and a tab → each language | each snippet escapes for its own language; run Python/JS/PHP and confirm the body arrives byte-identical |
| R7 | Emoji + a non-BMP character in the body → PHP and TypeScript | the character survives; PHP keeps it as literal UTF-8 |
| R8 | A HEAD request → **PHP** | both `CURLOPT_CUSTOMREQUEST => "HEAD"` and `CURLOPT_NOBODY => true` |
| R9 | Two headers with the same name (Auth → Custom, or two header rows) → **JavaScript** | `headers:` is an **array of pairs** and both values are there |
| R10 | Same request → **TypeScript** | `const headers: [string, string][]`, both values present |
| R11 | Same request → **Python** | a `# NOTE: requests takes headers as a mapping…` comment naming the repeated header(s) above the dict |
| R12 | Two form-data parts with the same key → **Python** | `files = [(...), (...)]` (a list, not a dict), both parts present |
| R13 | Same → **JavaScript** / **TypeScript** | two `form.append("key", …)` calls |
| R14 | Same → **PHP** | a `// NOTE: a PHP array has unique keys…` comment naming the repeated part(s) |
| R15 | Same → **cURL**, **Go**, **Rust** | both duplicates present, no note needed |
| R16 | Multipart with a file part → **PHP** | `new CURLFile('/path')`; run it and confirm the upload lands |
| R17 | **Copy cURL** (top bar) on any request, paste in a terminal | the command runs and returns the same response |
| R18 | Set Code to **Variables: {{kept}}**, then **Copy cURL** | the command keeps `{{placeholders}}` |
| R19 | Use a secret environment variable in a header, Code on **Variables: values**, **Copy cURL** | status line warns the command contains secret values |
| R20 | **Copy cURL** with an empty URL | red "Copy as cURL: …" explaining why, nothing copied |
| R21 | **Copy cURL**, then `+ cURL` in a new tab (paste it back) | the round trip reproduces method, URL, headers and body |
| R22 | Regression: doc 12's cURL/Python/JS/Go/Rust snippets | unchanged from before |

Report pass/fail per row; paste the TypeScript and PHP snippets from R2/R3 and whatever R4–R7 printed.
