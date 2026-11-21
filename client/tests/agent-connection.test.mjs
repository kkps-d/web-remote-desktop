import assert from "node:assert/strict"
import test from "node:test"
import { connectToAgent, parseAgentAddress, scaleToHeight } from "../src/lib/agent-connection.ts"

test("parses addresses, adding the agent's port only when none was typed", () => {
  for (const [typed, href] of [
    ["192.168.1.20", "http://192.168.1.20:8080/"],
    ["  pc.local:9000  ", "http://pc.local:9000/"],
    ["PC", "http://pc:8080/"],
    ["pc/path", "http://pc:8080/path"],
    ["http://pc:80", "http://pc/"],
    ["https://pc", "https://pc:8080/"],
    ["[::1]", "http://[::1]:8080/"],
    ["[::1]:9000", "http://[::1]:9000/"],
  ]) {
    assert.equal(parseAgentAddress(typed).href, href, typed)
  }
})

test("scales to a height with an even width", () => {
  assert.deepEqual(scaleToHeight({ width: 1920, height: 1080 }, 720), { width: 1280, height: 720 })
  assert.deepEqual(scaleToHeight({ width: 2560, height: 1080 }, 720), { width: 1706, height: 720 })
  assert.deepEqual(scaleToHeight({ width: 1366, height: 768 }, 720), { width: 1280, height: 720 })
})

class FakePeerConnection {
  static last
  iceGatheringState = "complete"
  steps = []
  closed = false
  onRemoteDescription = () => {}

  constructor() {
    FakePeerConnection.last = this
  }
  addTransceiver(kind) {
    this.steps.push(`transceiver ${kind}`)
  }
  createDataChannel(label) {
    this.steps.push(`channel ${label}`)
    return { label }
  }
  async createOffer() {
    this.steps.push("offer")
    return { type: "offer", sdp: "offer-sdp" }
  }
  async setLocalDescription(description) {
    this.localDescription = description
  }
  async setRemoteDescription(description) {
    this.remoteDescription = description
    this.onRemoteDescription()
  }
  addEventListener() {}
  close() {
    this.closed = true
  }
}

globalThis.RTCPeerConnection = FakePeerConnection
globalThis.MediaStream = class {
  addTrack() {}
}

/** Replaces fetch for one test; `respond` gets each request and returns a Response or a promise of one. */
function stubFetch(t, respond) {
  const requests = []
  t.mock.method(globalThis, "fetch", async (url, init = {}) => {
    const request = { url: String(url), method: init.method ?? "GET", init }
    requests.push(request)
    return respond(request)
  })
  return requests
}

const answer = () =>
  new Response("answer-sdp", { status: 201, headers: { Location: "/sessions/abc" } })

function agent(request) {
  if (request.url.endsWith("/offer")) return answer()
  if (request.method === "DELETE") return new Response(null, { status: 204 })
  return Response.json({ settings: { width: 1920 } })
}

test("creates the input channel before the offer so the SDP includes it", async (t) => {
  stubFetch(t, agent)
  const connection = await connectToAgent("pc", new AbortController().signal)

  assert.deepEqual(FakePeerConnection.last.steps, [
    "transceiver video",
    "transceiver audio",
    "channel input",
    "offer",
  ])
  assert.equal(connection.input.label, "input")
})

test("posts the offer and ends the session at the URL the agent returned", async (t) => {
  const requests = stubFetch(t, agent)
  const connection = await connectToAgent("pc:9000", new AbortController().signal)

  assert.equal(requests[0].url, "http://pc:9000/offer")
  assert.equal(requests[0].init.headers["Content-Type"], "application/sdp")
  assert.equal(requests[0].init.body, "offer-sdp")
  assert.deepEqual(FakePeerConnection.last.remoteDescription, { type: "answer", sdp: "answer-sdp" })

  connection.close()
  assert.equal(FakePeerConnection.last.closed, true)
  assert.equal(requests[1].url, "http://pc:9000/sessions/abc")
  assert.equal(requests[1].method, "DELETE")
  assert.equal(requests[1].init.keepalive, true)
})

test("cancelling before the offer is sent leaves the agent's current viewer alone", async (t) => {
  const requests = stubFetch(t, agent)
  const attempt = new AbortController()
  attempt.abort()

  await assert.rejects(connectToAgent("pc", attempt.signal), { name: "AbortError" })
  assert.equal(requests.length, 0)
  assert.equal(FakePeerConnection.last.closed, true)
})

test("cancelling after the agent answered ends the session it started", async (t) => {
  const requests = stubFetch(t, agent)
  const attempt = new AbortController()
  const connecting = connectToAgent("pc", attempt.signal)
  FakePeerConnection.last.onRemoteDescription = () => attempt.abort()

  await assert.rejects(connecting, { name: "AbortError" })
  assert.deepEqual(
    requests.map((request) => `${request.method} ${request.url}`),
    ["POST http://pc:8080/offer", "DELETE http://pc:8080/sessions/abc"],
  )
})

test("explains why the agent couldn't be reached or refused the offer", async (t) => {
  const responses = [
    [() => Promise.reject(new TypeError("fetch failed")), /Couldn't reach the agent at pc:8080/],
    [
      () => new Response("The offer has no constrained-baseline H.264 codec.", { status: 400 }),
      /no constrained-baseline/,
    ],
    [() => new Response("", { status: 500 }), /The agent responded with 500\./],
  ]
  for (const [respond, message] of responses) {
    t.mock.restoreAll()
    stubFetch(t, respond)
    await assert.rejects(connectToAgent("pc", new AbortController().signal), { message })
    assert.equal(FakePeerConnection.last.closed, true)
  }
})

test("explains failed session requests", async (t) => {
  let respond
  stubFetch(t, (request) => (request.url.endsWith("/offer") ? answer() : respond()))
  const connection = await connectToAgent("pc", new AbortController().signal)

  for (const [response, message] of [
    [() => new Response("", { status: 404 }), "The stream session has ended."],
    [() => Promise.reject(new TypeError("fetch failed")), "Couldn't reach the agent."],
    [
      () => new Response("Frame rate must be 1–120 fps.", { status: 400 }),
      "Frame rate must be 1–120 fps.",
    ],
  ]) {
    respond = response
    await assert.rejects(connection.updateStreamSettings({ framerate: 0 }), { message })
  }
})

test("fails session requests when the agent gave no session URL", async (t) => {
  stubFetch(t, () => new Response("answer-sdp", { status: 201 }))
  const connection = await connectToAgent("pc", new AbortController().signal)

  await assert.rejects(connection.getStreamInfo(), {
    message: "The agent didn't return a session URL.",
  })
})

test("sends settings requests one at a time, in order, even after a failure", async (t) => {
  const pending = []
  const requests = stubFetch(t, (request) =>
    request.url.endsWith("/offer") ? answer() : new Promise((resolve) => pending.push(resolve)),
  )
  const connection = await connectToAgent("pc", new AbortController().signal)
  const patches = () =>
    requests.filter((request) => request.method === "PATCH").map((request) => request.init.body)

  const first = connection.updateStreamSettings({ framerate: 60 })
  const second = connection.updateStreamSettings({ framerate: 30 })
  await new Promise(setImmediate)
  assert.deepEqual(patches(), ['{"framerate":60}'])
  assert.equal(requests[1].url, "http://pc:8080/sessions/abc")
  assert.equal(requests[1].init.headers["Content-Type"], "application/json")

  pending.shift()(new Response("Encoder failed", { status: 500 }))
  await assert.rejects(first, { message: "Encoder failed" })
  await new Promise(setImmediate)
  assert.deepEqual(patches(), ['{"framerate":60}', '{"framerate":30}'])

  assert.equal(requests[2].url, "http://pc:8080/sessions/abc")
  assert.equal(requests[2].init.headers["Content-Type"], "application/json")

  pending.shift()(Response.json({ settings: { framerate: 30 } }))
  assert.deepEqual(await second, { settings: { framerate: 30 } })
})

test("reads stream info and encoding stats from the session URL", async (t) => {
  const requests = stubFetch(t, agent)
  const connection = await connectToAgent("pc", new AbortController().signal)

  await connection.getStreamInfo()
  await connection.getEncodingStats()
  assert.deepEqual(
    requests.slice(1).map((request) => `${request.method} ${request.url}`),
    ["GET http://pc:8080/sessions/abc", "GET http://pc:8080/sessions/abc/stats"],
  )
})
