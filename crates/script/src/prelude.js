// The `pm` API for request scripts (a Postman-compatible subset), built on plain JavaScript.
// Rust puts the inputs in `__input` (JSON) and reads the results back from `__finish()`.
(function () {
  "use strict";
  const input = JSON.parse(globalThis.__input);
  delete globalThis.__input;
  const logs = [];
  const tests = [];
  const has = (o, k) => Object.prototype.hasOwnProperty.call(o, k);

  // ---- console ----
  const show = (v) => {
    if (typeof v === "string") return JSON.stringify(v);
    if (v === undefined) return "undefined";
    if (typeof v === "function") return "[function]";
    try {
      const s = JSON.stringify(v);
      return s === undefined ? String(v) : s;
    } catch (e) {
      return String(v);
    }
  };
  const fmt = (args) =>
    args.map((a) => (typeof a === "string" ? a : show(a))).join(" ");
  const log = (level) => (...args) => {
    if (logs.length < 500) logs.push([level, fmt(args).slice(0, 10000)]);
  };
  globalThis.console = {
    log: log("log"),
    info: log("info"),
    warn: log("warn"),
    error: log("error"),
    debug: log("log"),
  };

  // ---- variables ----
  const env = input.environment;
  const glob = input.globals;
  const vars = input.variables;
  const asText = (v) =>
    v === undefined || v === null
      ? ""
      : typeof v === "string"
        ? v
        : typeof v === "object"
          ? JSON.stringify(v)
          : String(v);
  const lookup = (n) => {
    for (const o of [vars, env, glob]) if (has(o, n)) return o[n];
    return undefined;
  };
  const replaceIn = (s) =>
    String(s).replace(/\{\{\s*([^{}]+?)\s*\}\}/g, (m, n) => {
      const v = lookup(n);
      return v === undefined ? m : v;
    });
  const scope = (obj, extra) =>
    Object.assign(
      {
        get: (k) => (has(obj, k) ? obj[k] : undefined),
        set: (k, v) => {
          obj[String(k)] = asText(v);
        },
        unset: (k) => {
          delete obj[k];
        },
        has: (k) => has(obj, k),
        clear: () => {
          for (const k of Object.keys(obj)) delete obj[k];
        },
        toObject: () => Object.assign({}, obj),
        replaceIn,
      },
      extra || {},
    );

  // ---- headers ----
  const headerList = (pairs) => {
    const h = pairs.map(([key, value]) => ({ key, value }));
    const find = (k) =>
      h.findIndex((x) => x.key.toLowerCase() === String(k).toLowerCase());
    return {
      get: (k) => {
        const i = find(k);
        return i < 0 ? undefined : h[i].value;
      },
      has: (k) => find(k) >= 0,
      add: (o) => {
        h.push({ key: String(o.key), value: asText(o.value) });
      },
      upsert: (o) => {
        const i = find(o.key);
        if (i < 0) h.push({ key: String(o.key), value: asText(o.value) });
        else h[i].value = asText(o.value);
      },
      remove: (k) => {
        let i;
        while ((i = find(k)) >= 0) h.splice(i, 1);
      },
      all: () => h.map((x) => ({ key: x.key, value: x.value })),
      each: (f) => h.forEach((x) => f({ key: x.key, value: x.value })),
      count: () => h.length,
      toObject: () => {
        const r = {};
        for (const x of h) r[x.key] = x.value;
        return r;
      },
      _list: h,
    };
  };

  // ---- assertions ----
  class AssertionError extends Error {
    constructor(message) {
      super(message);
      this.name = "AssertionError";
    }
  }
  const deepEq = (a, b) => {
    if (a === b) return true;
    if (typeof a === "number" && typeof b === "number") return a !== a && b !== b;
    if (a === null || b === null || typeof a !== "object" || typeof b !== "object")
      return false;
    if (Array.isArray(a) !== Array.isArray(b)) return false;
    const ka = Object.keys(a);
    const kb = Object.keys(b);
    if (ka.length !== kb.length) return false;
    return ka.every((k) => has(b, k) && deepEq(a[k], b[k]));
  };
  const typeName = (v) =>
    Array.isArray(v) ? "array" : v === null ? "null" : typeof v;
  const lengthOf = (v) =>
    v === null || v === undefined
      ? undefined
      : typeof v === "string" || Array.isArray(v)
        ? v.length
        : typeof v === "object"
          ? Object.keys(v).length
          : undefined;

  const expect = (actual, note) => {
    const st = { neg: false, deep: false };
    const a = {};
    const pre = note ? note + ": " : "";
    const check = (ok, msg, negMsg) => {
      if (st.neg ? ok : !ok) throw new AssertionError(pre + (st.neg ? negMsg : msg));
      return a;
    };
    const getter = (name, fn) =>
      Object.defineProperty(a, name, { get: fn, enumerable: false });
    for (const w of ["to", "be", "been", "is", "that", "which", "and", "has", "have", "with", "at", "of", "same", "does", "also"])
      getter(w, () => a);
    getter("not", () => {
      st.neg = !st.neg;
      return a;
    });
    getter("deep", () => {
      st.deep = true;
      return a;
    });
    const A = show(actual);
    a.equal = a.equals = a.eq = (e) =>
      check(
        st.deep ? deepEq(actual, e) : actual === e,
        `expected ${A} to equal ${show(e)}`,
        `expected ${A} to not equal ${show(e)}`,
      );
    a.eql = (e) =>
      check(deepEq(actual, e), `expected ${A} to deeply equal ${show(e)}`, `expected ${A} to not deeply equal ${show(e)}`);
    getter("true", () => check(actual === true, `expected ${A} to be true`, `expected ${A} to not be true`));
    getter("false", () => check(actual === false, `expected ${A} to be false`, `expected ${A} to not be false`));
    getter("null", () => check(actual === null, `expected ${A} to be null`, `expected ${A} to not be null`));
    getter("undefined", () => check(actual === undefined, `expected ${A} to be undefined`, `expected ${A} to not be undefined`));
    getter("NaN", () => check(actual !== actual, `expected ${A} to be NaN`, `expected ${A} to not be NaN`));
    getter("ok", () => check(!!actual, `expected ${A} to be truthy`, `expected ${A} to be falsy`));
    getter("exist", () => check(actual !== null && actual !== undefined, `expected ${A} to exist`, `expected ${A} to not exist`));
    getter("empty", () => check(lengthOf(actual) === 0, `expected ${A} to be empty`, `expected ${A} to not be empty`));
    a.above = a.gt = a.greaterThan = (n) => check(actual > n, `expected ${A} to be above ${n}`, `expected ${A} to be at most ${n}`);
    a.below = a.lt = a.lessThan = (n) => check(actual < n, `expected ${A} to be below ${n}`, `expected ${A} to be at least ${n}`);
    a.least = a.gte = (n) => check(actual >= n, `expected ${A} to be at least ${n}`, `expected ${A} to be below ${n}`);
    a.most = a.lte = (n) => check(actual <= n, `expected ${A} to be at most ${n}`, `expected ${A} to be above ${n}`);
    a.within = (lo, hi) => check(actual >= lo && actual <= hi, `expected ${A} to be within ${lo}..${hi}`, `expected ${A} to not be within ${lo}..${hi}`);
    a.a = a.an = (t) =>
      check(typeName(actual) === String(t).toLowerCase(), `expected ${A} to be a ${t}`, `expected ${A} to not be a ${t}`);
    a.include = a.includes = a.contain = a.contains = (x) => {
      let ok = false;
      if (typeof actual === "string") ok = actual.includes(String(x));
      else if (Array.isArray(actual)) ok = actual.some((y) => deepEq(y, x));
      else if (actual && typeof actual === "object" && x && typeof x === "object")
        ok = Object.keys(x).every((k) => deepEq(actual[k], x[k]));
      return check(ok, `expected ${A} to include ${show(x)}`, `expected ${A} to not include ${show(x)}`);
    };
    a.property = (name, ...val) => {
      const present = actual !== null && actual !== undefined && name in Object(actual);
      if (val.length === 0)
        return check(present, `expected ${A} to have property ${show(name)}`, `expected ${A} to not have property ${show(name)}`);
      return check(
        present && deepEq(actual[name], val[0]),
        `expected ${A} to have property ${show(name)} of ${show(val[0])}` +
          (present ? `, but got ${show(actual[name])}` : ""),
        `expected ${A} to not have property ${show(name)} of ${show(val[0])}`,
      );
    };
    a.lengthOf = a.length = (n) =>
      check(lengthOf(actual) === n, `expected ${A} to have length ${n} but got ${lengthOf(actual)}`, `expected ${A} to not have length ${n}`);
    a.match = (re) =>
      check(re.test(String(actual)), `expected ${A} to match ${re}`, `expected ${A} to not match ${re}`);
    a.oneOf = (list) =>
      check(list.some((x) => deepEq(x, actual)), `expected ${A} to be one of ${show(list)}`, `expected ${A} to not be one of ${show(list)}`);
    a.keys = (...ks) => {
      const want = Array.isArray(ks[0]) ? ks[0] : ks;
      const ok = actual && typeof actual === "object" && want.every((k) => has(actual, k));
      return check(ok, `expected ${A} to have keys ${show(want)}`, `expected ${A} to not have keys ${show(want)}`);
    };
    // expect(pm.response).to.have.status(200) / .header("X")
    a.status = (c) => {
      const got = actual && (typeof c === "number" ? actual.code : actual.status);
      return check(got === c, `expected response to have status ${show(c)} but got ${show(got)}`, `expected response to not have status ${show(c)}`);
    };
    a.header = (k, v) => {
      const hs = actual && actual.headers;
      const present = !!hs && hs.has(k);
      if (v === undefined)
        return check(present, `expected response to have header ${show(k)}`, `expected response to not have header ${show(k)}`);
      return check(present && hs.get(k) === v, `expected header ${show(k)} to be ${show(v)} but got ${show(hs && hs.get(k))}`, `expected header ${show(k)} to not be ${show(v)}`);
    };
    return a;
  };

  // ---- request / response ----
  const rq = input.request;
  const reqHeaders = headerList(rq.headers);
  const request = {
    method: rq.method,
    url: rq.url,
    headers: reqHeaders,
    body: { mode: "raw", raw: rq.body },
  };

  let response;
  if (input.response) {
    const r = input.response;
    const rh = headerList(r.headers);
    const fail = (m) => {
      throw new AssertionError(m);
    };
    const range = (lo, hi, what) => () =>
      r.code >= lo && r.code <= hi
        ? true
        : fail(`expected response to be ${what} but got status ${r.code}`);
    response = {
      code: r.code,
      status: r.status,
      responseTime: r.time_ms,
      responseSize: r.size,
      headers: rh,
      text: () => r.body,
      json: () => JSON.parse(r.body),
    };
    const ok = range(200, 299, "ok (2xx)");
    response.to = {
      have: {
        status: (c) => expect(response).to.have.status(c),
        header: (k, v) => expect(response).to.have.header(k, v),
        body: (s) =>
          s === undefined
            ? r.body.length > 0 || fail("expected response to have a body")
            : r.body === s || fail(`expected response body to be ${show(s)}`),
        jsonBody: (path) => {
          let j;
          try {
            j = JSON.parse(r.body);
          } catch (e) {
            fail("expected response to have a JSON body");
          }
          if (path !== undefined && !(path in Object(j)))
            fail(`expected JSON body to have ${show(path)}`);
          return true;
        },
      },
      be: {},
    };
    for (const [name, fn] of [
      ["ok", ok],
      ["success", ok],
      ["redirection", range(300, 399, "a redirect (3xx)")],
      ["clientError", range(400, 499, "a client error (4xx)")],
      ["serverError", range(500, 599, "a server error (5xx)")],
      ["notFound", range(404, 404, "not found (404)")],
      ["unauthorized", range(401, 401, "unauthorized (401)")],
      ["forbidden", range(403, 403, "forbidden (403)")],
      ["badRequest", range(400, 400, "a bad request (400)")],
    ])
      Object.defineProperty(response.to.be, name, { get: fn });
    response.to.not = {};
  }

  // ---- pm ----
  const test = (name, fn) => {
    try {
      if (typeof fn === "function") fn();
      tests.push({ name: String(name), passed: true, skipped: false, error: null });
    } catch (e) {
      tests.push({
        name: String(name),
        passed: false,
        skipped: false,
        error: e && e.message !== undefined ? String(e.message) : String(e),
      });
    }
  };
  test.skip = (name) =>
    tests.push({ name: String(name), passed: true, skipped: true, error: null });

  globalThis.pm = {
    environment: scope(env, { name: input.environment_name || "" }),
    globals: scope(glob),
    variables: scope(vars, {
      get: (k) => lookup(k),
      has: (k) => lookup(k) !== undefined,
    }),
    collectionVariables: scope({}),
    request,
    response,
    test,
    expect,
    info: { eventName: input.phase, requestName: input.request_name || "" },
    sendRequest: () => {
      throw new Error("pm.sendRequest isn't supported yet");
    },
  };

  // ---- legacy Postman globals ----
  globalThis.tests = {};
  globalThis.postman = {
    setEnvironmentVariable: (k, v) => pm.environment.set(k, v),
    getEnvironmentVariable: (k) => pm.environment.get(k),
    clearEnvironmentVariable: (k) => pm.environment.unset(k),
    setGlobalVariable: (k, v) => pm.globals.set(k, v),
    getGlobalVariable: (k) => pm.globals.get(k),
    clearGlobalVariable: (k) => pm.globals.unset(k),
  };
  if (input.response) {
    globalThis.responseBody = input.response.body;
    globalThis.responseCode = { code: input.response.code, name: input.response.status };
    globalThis.responseTime = input.response.time_ms;
    globalThis.responseHeaders = response.headers.toObject();
  }

  globalThis.__finish = () => {
    for (const [k, v] of Object.entries(globalThis.tests || {}))
      tests.push({ name: k, passed: !!v, skipped: false, error: v ? null : "failed" });
    return JSON.stringify({
      environment: env,
      globals: glob,
      variables: vars,
      request: {
        method: String(request.method),
        url: String(request.url),
        headers: reqHeaders._list.map((x) => [x.key, x.value]),
        body: asText(request.body && request.body.raw),
      },
      tests,
      logs,
    });
  };
})();
