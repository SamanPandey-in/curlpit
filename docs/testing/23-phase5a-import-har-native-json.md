# 23 — Phase 5a: import HAR and native JSON, one import path

> Built and compiled on your Mac over SSH (`fmt`, `clippy -D warnings`, unit tests, `at-app` clippy and tests). The GUI hasn't been clicked through yet; that's this checklist. Nothing visual changed, so it is short.

**New**
- **HAR import (FR-IE-03).** Import… now accepts a `.har` file, for example one saved from your browser's DevTools. It becomes one collection named **Imported from HAR**, one request per entry, each named `METHOD path`. Query strings become Params, headers come across (HTTP/2 pseudo-headers such as `:authority` are dropped), and the body is mapped from its content type: JSON, XML, HTML, plain text, form-urlencoded and multipart.
- **Native JSON import and export (FR-IE-04, FR-IE-06), in the library.** A versioned, lossless JSON form of a collection (`{ "format": "api-tester-collection", "version": 1, ... }`). Importing it back gives exactly what went in.
- **One import path that works out what a file is (FR-IE-09).** You never choose a format: Postman collection, Postman environment or globals, HAR and native JSON are all recognised from the file's contents. A UTF-8 BOM, common on files saved by Windows tools, is accepted for every format. A file that is none of these gets a clear message naming what is accepted, and nothing is created.
- **18 new tests**: 7 HAR, 5 native JSON, 6 for the dispatcher, including the phase exit criterion (a Postman collection survives export and re-import with every folder and request intact) and a check that every source format survives native JSON unchanged. I confirmed the BOM test fails when the BOM handling is removed.

**Not yet:** a button for native JSON export, drag and drop, and paste for collections. The three need `render/mod.rs`, which the `ui` branch also changes, so I held them back to keep that merge clean. Until they land **native JSON can't be produced from the GUI**, so rows below cover HAR and Postman only. Also not yet: previewing what will be imported before confirming (FR-IE-10), OpenAPI and YAML (P1).

## Known limits, stated plainly
- **HAR has no folders**, so every request lands at the top level of the collection.
- **HAR keeps auth as headers.** An `Authorization` header stays a header; it is not turned into the Auth tab.
- **A HAR file is parsed twice** (once to recognise it, once to read it). Fine for normal files; if a very large capture feels slow, that's why. Recorded in D42.
- **A HAR with browser noise** (cookies, tracking calls) imports all of it. Delete what you don't want in the sidebar.

## Setup
```
cd ~/inCoding/nextgen/api-tester
cargo test                      # expect 167 passed
cargo test -p at-interop        # expect 31 passed
cargo test -p at-app            # expect 13 passed
API_TESTER_DATA_DIR=/tmp/at-test15 cargo run --release -p at-app
```
For a real HAR: in Chrome, open DevTools, Network, reload a page, right-click the list, **Save all as HAR with content**.

## Checklist
| # | Do | Expect |
|---|---|---|
| I1 | Collections → **Import…**, pick a real Postman v2.1 export | folders and requests appear in the sidebar; status says how many requests and folders (regression: unchanged) |
| I2 | Import… and pick a `.har` saved from your browser | a collection **Imported from HAR** appears, one request per entry, named like `GET /api/users` |
| I3 | Open a GET from it that had a query string | the Params tab lists the query parameters, and the URL matches the captured one |
| I4 | Open a POST that sent JSON | the Body tab shows JSON with the captured body |
| I5 | Open any request and look at Headers | headers are there; none start with `:` (`:authority`, `:method`, ...) |
| I6 | Send one imported request | it sends like any other (regression: nothing special about imported ones) |
| I7 | Save a Postman or HAR file as **UTF-8 with BOM** (Notepad: Save As, Encoding), then import it | imports normally, no JSON error |
| I8 | Import… and pick a plain text file, then a truncated JSON file | a red message `Could not import …`; the app does not crash; no collection is created |
| I9 | Import… and pick some other JSON file (a `package.json` will do) | message says it is not a recognised import file and lists Postman, HAR and native JSON |
| I10 | Import a Postman **v1** collection if you have one | the specific message to open it in Postman and re-export as v2.1 |
| I11 | Variables page → **Import…** with a Postman environment, then a globals file | still imports as an environment or globals (regression) |
| I12 | **Exit criterion.** Export a collection (Export…), delete it, import the file you just saved | the same folders and requests come back |
| I13 | Anything that looks wrong | say what and where |

Report pass/fail per row.
