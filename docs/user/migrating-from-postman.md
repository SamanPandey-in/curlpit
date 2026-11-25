# Migrating from Postman

Curlpit reads Postman's export files directly. This page covers what comes across, what changes shape on the way, and what doesn't come across yet. Every import ends with a status line listing **notes** for anything that couldn't be represented. Nothing is dropped without a note, apart from the few items listed under [Known gaps](#known-gaps).

## Export from Postman

- **Collection:** collection menu → *Export* → **Collection v2.1** (v2.0 also works). Postman v1 files are refused with a message asking you to re-export as v2.1.
- **Environment:** *Environments* → environment menu → *Export*.
- **Globals:** *Environments* → *Globals* → *Export*.

## Import into Curlpit

Use any of these. They all go through the same importer, and the file type is detected from its contents:

- drop one or more files onto the window;
- **Import…** at the bottom of the Collections sidebar (or on the Variables page), or *Import file…* in the command palette;
- copy the JSON and press **Paste** next to Import, or run *Import from clipboard* from the palette.

| You import | You get |
|---|---|
| A collection | A **new** collection folder on disk. Importing the same file twice gives a second copy (`name-2/`); nothing is merged or overwritten. |
| An environment | A new environment. A name that already exists gets ` 2`. |
| Globals | Merged into your globals by key: same key replaces, new keys are added. |

Curlpit also imports HAR files (as one flat collection named "Imported from HAR"), its own JSON export, and single cURL commands (**Import cURL** in the tab strip reads the clipboard).

## What converts

- Method, URL, headers, and their descriptions and enabled/disabled flags.
- **Query parameters** move into the Params tab (the `?…` part is removed from the URL, so nothing is sent twice). Disabled parameters stay disabled.
- **Bodies:** raw JSON, XML, HTML and text (the kind comes from Postman's language setting), `x-www-form-urlencoded`, and form-data including file fields (as file paths). **GraphQL** bodies are kept as GraphQL, and sent as a JSON `{"query", "variables"}` body.
- **Auth:** none, Basic, Bearer, API key (header or query). OAuth 2.0 and Digest settings are imported and kept, but can't be sent yet (a note says so). OAuth 2.0 access tokens are not imported; you'll fetch a new token once OAuth sending exists.
- **Scripts:** pre-request and test scripts (`event` blocks) come across as JavaScript. See [Scripts](#scripts) for what runs.
- **Settings:** Postman's *follow redirects* and *SSL certificate verification* per request.
- **Folders** keep their real names (`User Management/`, not a slug).
- **Environment variables** keep their enabled flag. Postman `secret`-type values are marked secret (masked in the editor).

## What is flattened

Curlpit has no folder- or collection-level auth or scripts that apply at send time yet. Postman's are pushed down into each request (decision D28 in [DECISIONS.md](../DECISIONS.md)):

- **Auth inheritance:** a request set to *inherit* gets its parent folder's auth, or the collection's, copied into its own file. A request with *No Auth* stays without auth.
- **Scripts:** collection- and folder-level scripts are **prepended** to each request's own script, outermost first, which matches Postman's run order. The import note says how many levels were copied.

So after import, changing a shared token means editing each request, or better, putting the token in an environment variable (`{{token}}`) and editing it once. Curlpit can already read and write `collection.toml` and `folder.toml` (see [collections on disk](collections-on-disk.md)), but it doesn't apply them when sending yet.

## Scripts

Curlpit runs scripts in a sandboxed QuickJS engine with a Postman-compatible subset of `pm`. Most everyday test scripts run unchanged:

```js
pm.test("status is 200", () => pm.response.to.have.status(200));
pm.test("has an id", () => pm.expect(pm.response.json()).to.have.property("id"));
pm.environment.set("token", pm.response.json().token);
```

Supported: `pm.test`, `pm.expect` (Chai-style), `pm.response` (code, status, headers, `json()`, `text()`, `to.have.status`, `to.be.ok` and friends), `pm.request` (URL, method, headers, raw body; pre-request changes apply to the sent request), `pm.environment`, `pm.globals`, `pm.variables`, `console.*`, and the legacy `tests[...]`, `responseBody` and `postman.setEnvironmentVariable` forms.

**Not supported yet.** A script that uses these fails with an error on the Tests tab:

- `pm.sendRequest` (throws "pm.sendRequest isn't supported yet")
- `require(...)`, which means no lodash, moment, CryptoJS, `uuid` and so on
- `pm.cookies`, `pm.iterationData`, `pm.info.iteration`
- `pm.collectionVariables` exists but always starts empty and isn't saved
- `postman.setNextRequest`, async tests, and `atob` / `btoa`

Each script is limited to 5 seconds and 64 MB. The [scripting reference](scripting.md) lists every supported call.

## Variables

`{{name}}` works the same as in Postman, in the URL, params, headers, body and auth. Precedence, highest first: `pm.variables` set by a script during this send, then the active environment, then globals.

Differences:

- **Collection variables** are not imported. The note says how many there were; move them into an environment.
- **Dynamic variables** such as `{{$guid}}`, `{{$timestamp}}` and `{{$randomInt}}` aren't supported, and stay as literal text. Use a pre-request script instead, for example `pm.variables.set("ts", String(Date.now()))`.
- **Path variables** (`/users/:id`) are kept as text with a note. Replace them with `{{id}}`.

## Known gaps

- Auth types other than none, Basic, Bearer, API key, OAuth 2.0 and Digest (AWS Signature, Hawk, NTLM, Akamai and others) are imported **without auth**, with a note.
- Binary-file bodies are not imported (noted).
- Request-level descriptions and saved example responses are not imported.
- **Request order:** Curlpit lists folders first, then requests by name. Postman's manual order is not kept, which also affects the collection runner's order.
- An unsupported HTTP method becomes GET, with a note.
- You can't edit OAuth 2.0, Digest, JWT or custom-header auth in the app yet. They are kept and shown read-only.

## Going back

You can export to Postman at any time: **Export…** in the sidebar (Postman v2.1), or *Export collection as Postman…* in the command palette. Scripts, params (including disabled ones), bodies and the supported auth types export. JWT and custom-header auth export as plain headers. Per-request timeouts have no Postman equivalent, and the export notes say so. Environment export leaves secret values empty unless you turn on **Include secrets**.
