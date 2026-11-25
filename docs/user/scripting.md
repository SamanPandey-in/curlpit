# Scripting reference

Each request can have two JavaScript scripts, edited on its **Scripts** tab:

- **Pre-request** runs before the request is built and sent. It can read and set variables, and change the request's URL, headers and raw body for this send.
- **Tests** runs after the response arrives. Results show on the response's **Tests** tab as ✓ (passed), ✗ (failed, with the message) or ○ (skipped), followed by the script's console output.

The API is a Postman-compatible subset called `pm`. Everything on this page is taken from [`crates/script/src/prelude.js`](../../crates/script/src/prelude.js), which is the whole implementation. If something isn't listed here, it doesn't exist.

## The sandbox

- The engine is QuickJS with ES2023 built-ins (`JSON`, `Math`, `Date`, `RegExp`, `Map`, `Promise` and so on).
- There is no file, network or process access: `require`, `fetch`, `XMLHttpRequest` and `process` are all undefined.
- **Limits per script run: 5 seconds, 64 MB of memory and a 512 KB stack.** A script that runs too long stops with "Script stopped: it ran longer than 5 s". Tests that finished before the limit are still reported.
- Every run starts with a fresh engine, so nothing carries over between runs except variables (see below).
- Scripts run off the UI thread, so a slow script never freezes the window.
- A syntax error or uncaught exception is shown in red with its line number. The request itself is unaffected: an error in a test script doesn't change the response you got.

## Variables

There are three scopes, plus a combined view:

| Object | Scope | Persisted? |
|---|---|---|
| `pm.environment` | The active environment | Yes. Changes are saved into the active environment. With no active environment they are dropped and a note says so. |
| `pm.globals` | Globals | Yes |
| `pm.variables` | Local to this send | No. It lasts for this one send, from the pre-request script through the tests. |

Each of these objects has the same methods:

| Method | Does |
|---|---|
| `.get(name)` | The value, or `undefined` |
| `.set(name, value)` | Sets a value. Non-strings are stored as text; objects become JSON. |
| `.unset(name)` | Removes it |
| `.has(name)` | `true` / `false` |
| `.clear()` | Removes every variable in that scope |
| `.toObject()` | A plain `{name: value}` copy |
| `.replaceIn(text)` | Replaces `{{name}}` in `text`, looking through local, then environment, then global variables. Unknown names are left as they are. |

Special cases:

- `pm.variables.get(name)` and `pm.variables.has(name)` look through **all** scopes (local, then environment, then global). `pm.variables.set` always writes the local scope.
- `pm.environment.name` is the active environment's name (`""` if none).
- `pm.collectionVariables` has the same methods but always starts empty and is never saved.

Variables set in a pre-request script apply to the same send. For example, set a token with `pm.environment.set("token", …)` and `{{token}}` in the Auth tab resolves to it. When environment or global variables are saved, only the changed keys are written: secret flags and disabled variables are kept, and two tabs changing different variables at the same time don't undo each other.

## `pm.request`

The request as typed, with variables **not** resolved yet, and enabled headers only.

| Member | Notes |
|---|---|
| `pm.request.url` | String; assign to change it |
| `pm.request.method` | String |
| `pm.request.body.raw` | Raw body text (empty for urlencoded and form-data bodies); assign to change it |
| `pm.request.headers.get(name)` | Case-insensitive |
| `pm.request.headers.has(name)` | |
| `pm.request.headers.add({key, value})` | Appends, even if the name exists |
| `pm.request.headers.upsert({key, value})` | Replaces the first header with that name, or adds one |
| `pm.request.headers.remove(name)` | Removes every header with that name |
| `pm.request.headers.all()` | `[{key, value}, …]` |
| `pm.request.headers.each(fn)` | Calls `fn({key, value})` for each header |
| `pm.request.headers.count()` | |
| `pm.request.headers.toObject()` | `{name: value}` (a later duplicate wins) |

Changes made in a **pre-request** script apply to the request that is sent, never to the editor or the saved file. Form bodies (urlencoded, form-data) can't be changed by scripts.

## `pm.response` (test scripts only)

| Member | Notes |
|---|---|
| `pm.response.code` | Status code, such as `200` |
| `pm.response.status` | Status text, such as `"OK"` |
| `pm.response.responseTime` | Milliseconds |
| `pm.response.responseSize` | Bytes |
| `pm.response.headers` | Same methods as `pm.request.headers` |
| `pm.response.text()` | The body as text |
| `pm.response.json()` | The body parsed as JSON (throws if it isn't JSON) |

The body passed to scripts is capped at 10 MB. Responses large enough to be spilled to disk (over 10 MB) reach the test script as an empty body.

Assertions on the response throw on failure, so use them inside `pm.test`:

| Call | Passes when |
|---|---|
| `pm.response.to.have.status(200)` | The code is 200 (a number compares the code; a string compares the status text) |
| `pm.response.to.have.header("X")` / `.header("X", "v")` | The header exists / has that value |
| `pm.response.to.have.body()` / `.body("text")` | The body is non-empty / equals the text exactly |
| `pm.response.to.have.jsonBody()` / `.jsonBody("key")` | The body parses as JSON / has that top-level key |
| `pm.response.to.be.ok`, `.success` | 2xx |
| `pm.response.to.be.redirection` | 3xx |
| `pm.response.to.be.clientError` / `.serverError` | 4xx / 5xx |
| `pm.response.to.be.badRequest`, `.unauthorized`, `.forbidden`, `.notFound` | 400, 401, 403, 404 |

`pm.response.to.not` exists but is empty, so negated response assertions are not available. Use `pm.expect(pm.response.code).to.not.equal(500)` instead.

## Tests

```js
pm.test("name", () => { /* throw, or fail an assertion, to fail */ });
pm.test.skip("name");          // reported as skipped (○); the function is not run
```

A test passes if its function returns without throwing. Async tests are not supported: a test can't wait for a promise or a timer.

Legacy form, still supported: `tests["name"] = booleanExpression;`. Each key becomes a test that passes when the value is truthy.

## `pm.expect` (Chai-style)

`pm.expect(actual)` or `pm.expect(actual, "message prefix")`, then a chain.

**Words that only read well** (they do nothing): `to`, `be`, `been`, `is`, `that`, `which`, `and`, `has`, `have`, `with`, `at`, `of`, `same`, `does`, `also`.

**Modifiers:** `not` (negates what follows), `deep` (makes `equal` compare deeply).

**Property-style assertions** (no parentheses): `true`, `false`, `null`, `undefined`, `NaN`, `ok` (truthy), `exist` (not null or undefined), `empty` (length or key count is 0).

**Method assertions:**

| Method (aliases) | Passes when |
|---|---|
| `equal(x)` (`equals`, `eq`) | `actual === x`, or deep-equal after `.deep` |
| `eql(x)` | Deep-equal |
| `above(n)` (`gt`, `greaterThan`) | `actual > n` |
| `below(n)` (`lt`, `lessThan`) | `actual < n` |
| `least(n)` (`gte`) | `actual >= n` |
| `most(n)` (`lte`) | `actual <= n` |
| `within(lo, hi)` | `lo <= actual <= hi` |
| `a(type)` (`an`) | Type is `"string"`, `"number"`, `"boolean"`, `"object"`, `"array"`, `"null"`, `"undefined"` or `"function"` |
| `include(x)` (`includes`, `contain`, `contains`) | A string contains `x`; an array has an element deep-equal to `x`; an object has all of `x`'s key/values |
| `property(name)` / `property(name, value)` | The property exists / exists and deep-equals `value` |
| `lengthOf(n)` (`length`) | A string or array has length `n`, or an object has `n` keys |
| `match(regex)` | `regex.test(String(actual))` |
| `oneOf(list)` | `actual` deep-equals one element of `list` |
| `keys(a, b, …)` or `keys([a, b])` | The object has all of those own keys |
| `status(code)` | On `pm.expect(pm.response)`: the code (number) or status text (string) matches |
| `header(name[, value])` | On `pm.expect(pm.response)`: the header exists, or has that value |

Examples:

```js
const body = pm.response.json();
pm.test("shape", () => {
  pm.expect(body).to.have.property("id");
  pm.expect(body.tags).to.be.an("array").that.is.not.empty;  // `.an()` returns the chain
  pm.expect(body.name).to.match(/^[A-Z]/);
  pm.expect(body).to.deep.equal({ id: 1, name: "A", tags: ["x"] });
});
```

## `pm.info`

- `pm.info.eventName`: `"prerequest"` or `"test"`
- `pm.info.requestName`: the request's name

## Console

`console.log`, `console.info`, `console.warn`, `console.error` and `console.debug` (shown as log). Strings print as-is. Other values are printed as JSON. The last 500 lines are kept, each cut at 10,000 characters.

## Legacy Postman globals

| Global | Equivalent |
|---|---|
| `tests["name"] = bool` | `pm.test` |
| `responseBody` | `pm.response.text()` |
| `responseCode.code`, `responseCode.name` | `pm.response.code`, `pm.response.status` |
| `responseTime` | `pm.response.responseTime` |
| `responseHeaders` | `pm.response.headers.toObject()` |
| `postman.setEnvironmentVariable(k, v)` / `getEnvironmentVariable(k)` / `clearEnvironmentVariable(k)` | `pm.environment.set/get/unset` |
| `postman.setGlobalVariable(k, v)` / `getGlobalVariable(k)` / `clearGlobalVariable(k)` | `pm.globals.set/get/unset` |

## Not supported

| Feature | What happens |
|---|---|
| `pm.sendRequest(...)` | Throws "pm.sendRequest isn't supported yet" |
| `require(...)` (lodash, moment, CryptoJS, …) | `require` is undefined |
| `pm.cookies` | Undefined. The app's cookie jar works for requests, but scripts can't read it. |
| `pm.iterationData`, `pm.info.iteration` | Undefined |
| `pm.collectionVariables` persistence | In-memory only, empty at the start of each run |
| `pm.response.to.not.*`, `pm.response.to.be.json` and other Postman extras not listed above | Undefined |
| `postman.setNextRequest` | Undefined; the runner always goes in sidebar order |
| `atob` / `btoa`, `setTimeout`, async tests | Not available |
| Dynamic variables (`{{$guid}}`, `{{$timestamp}}`, …) | Left as literal text |

## Recipes

Save a token from a login response, then use it as `{{token}}` in later requests:

```js
pm.test("logged in", () => pm.response.to.have.status(200));
pm.environment.set("token", pm.response.json().token);
```

Add a header and a per-send value from a pre-request script:

```js
pm.variables.set("requestId", "req-" + Date.now());
pm.request.headers.upsert({ key: "X-Request-Id", value: "{{requestId}}" });
```

Check that the response is fast enough:

```js
pm.test("under 500 ms", () => pm.expect(pm.response.responseTime).to.be.below(500));
```
