# Quickstart

This guide takes you from a fresh build to a saved, tested request in a few minutes. Shortcuts are shown as `Ctrl` (Windows and Linux) / `Cmd` (macOS).

## 1. Build and start

```sh
cargo build -p at-app --release
./target/release/api-tester
```

The README's [Install](../../README.md#install) section lists the prerequisites for each OS. To try Curlpit without touching your real data, point it at a scratch folder:

```sh
API_TESTER_DATA_DIR=/tmp/curlpit-try ./target/release/api-tester
```

## 2. The sample collection

On the first start with an empty data directory, Curlpit creates:

- a collection, **Getting started**, with five requests against [httpbin.org](https://httpbin.org), a public service that echoes back what you send;
- an environment, **httpbin**, with one variable, `baseUrl = https://httpbin.org`, already selected in the title bar.

| Request | Shows |
|---|---|
| 1. GET with query params | Params: httpbin echoes them back under `args` |
| 2. POST JSON | A raw JSON body; `Content-Type: application/json` is added for you |
| 3. Header and variable | A custom header and a `{{baseUrl}}` URL |
| 4. Test script | `pm.test` checks shown in the response's **Tests** tab |
| 5. Pre-request script | A script that sets a variable used in a header, then a test that checks it was sent |

The sample is created once. If you delete it, it does not come back, and a data directory that already has anything in it (settings, history, environments or collections) is never seeded.

## 3. Send

Click **1. GET with query params** in the sidebar, then click **Send**, press `Ctrl+Enter` / `Cmd+Enter`, or press Enter in the URL box. The response pane shows the status, time and size, then:

- **Body:** the response, pretty-printed if it is JSON. **Wrap**, **Copy** and **Save…** are in the response bar, and `Ctrl+F` / `Cmd+F` finds text.
- **Tree:** a collapsible JSON tree. Select a node, then use **Copy value** or **Copy path** (for example `slideshow.slides[0].title`).
- **Headers:** the response headers.
- **Tests:** results of the request's test script, plus its `console.log` output.
- **Code:** the same request as cURL, Python, JavaScript, TypeScript, Go, PHP or Rust.

Pressing Send again while a request is running cancels it.

## 4. Use a variable

Open **3. Header and variable**. Its URL is `{{baseUrl}}/headers`. When you send it, `{{baseUrl}}` is replaced with the active environment's value. Switch the title-bar picker to **No environment** and send again: the unresolved name is listed in amber next to the status, and the request can't reach httpbin.

Manage variables with **Manage environments…** in the picker, or the **Variables** button. Variables work in the URL, params, headers, body and auth fields. If the same name exists in both, the active environment's value wins over the globals.

## 5. Make and save your own request

1. Press `Ctrl+T` / `Cmd+T` for a new tab.
2. Type `{{baseUrl}}/uuid` in the URL box and press Enter.
3. Give it a name in the name box and press `Ctrl+S` / `Cmd+S`.

Save writes into the collection or folder you last clicked in the sidebar. If you haven't clicked one, it saves into the first collection, or creates **My Collection** if there are none. Saving an opened request overwrites its file. Renaming it and saving moves the file.

## 6. Add a test

Open the **Scripts** tab and type into **Tests**:

```js
pm.test("status is 200", () => {
  pm.response.to.have.status(200);
});
```

Send, and the response's **Tests** tab shows the result. The **Insert** buttons add common snippets. See the [scripting reference](scripting.md) for the full API.

## 7. Run a whole collection

Click a collection or folder in the sidebar, then **Run…** at the bottom. Set the iterations, the delay and stop-on-failure, then press **Run**. Each request goes through the same pipeline as Send, and the report fills in live. Runs use the saved files, not unsaved tab edits, and they don't add to History.

## 8. Bring your Postman work

Export your collection from Postman (Collection v2.1) and either drop the file onto the Curlpit window, use **Import…** at the bottom of the sidebar, or copy the JSON and press **Paste**. Environments and globals import the same way. See [migrating from Postman](migrating-from-postman.md) for what converts.

## Where next

- [Keyboard shortcuts](shortcuts.md) and the command palette (`Ctrl+K` / `Cmd+K`)
- [Collections on disk](collections-on-disk.md), to version your requests with Git
- [Scripting reference](scripting.md)
