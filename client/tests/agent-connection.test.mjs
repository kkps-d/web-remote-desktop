import assert from "node:assert/strict"
import test from "node:test"
import {
  connectToAgent,
  connectToGame,
  launchAgentFromSearch,
  parseAgentAddress,
  scaleToHeight,
} from "../src/lib/agent-connection.ts"

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

test("localAgent only accepts loopback addresses for automatic connections", () => {
  const viewerHost = "192.168.1.20"
  assert.equal(launchAgentFromSearch("?localAgent=127.0.0.1%3A8100", viewerHost), "127.0.0.1:8100")
  assert.equal(launchAgentFromSearch("?localAgent=localhost%3A8100", viewerHost), "localhost:8100")
  for (const address of [
    "example.org:8100",
    "127.0.0.1",
    "localhost:0",
    "localhost:65536",
    "localhost:8100/path",
  ]) {
    assert.equal(
      launchAgentFromSearch(new URLSearchParams({ localAgent: address }).toString(), viewerHost),
      undefined,
    )
  }
  assert.equal(launchAgentFromSearch("", viewerHost), undefined)
})

test("agentPort opens the agent on the viewer's host", () => {
  const viewerHost = "192.168.1.20"
  assert.equal(launchAgentFromSearch("?agentPort=8100", viewerHost), "192.168.1.20:8100")
  for (const agentPort of ["0", "65536", "8e3", "8100/path", "other-host:8100"]) {
    assert.equal(
      launchAgentFromSearch(new URLSearchParams({ agentPort }).toString(), viewerHost),
      undefined,
    )
  }
})

test("scales to a height with an even width", () => {
  assert.deepEqual(scaleToHeight({ width: 1920, height: 1080 }, 720), { width: 1280, height: 720 })
  assert.deepEqual(scaleToHeight({ width: 2560, height: 1080 }, 720), { width: 1706, height: 720 })
  assert.deepEqual(scaleToHeight({ width: 1366, height: 768 }, 720), { width: 1280, height: 720 })
})

const relayOffer = "v=0\r\na=candidate:1 1 udp 1 172.30.42.2 49160 typ relay\r\n"
class FakePeerConnection {
  static last
  iceGatheringState = "complete"
  connectionState = "connected"
  steps = []
  closed = false
  onRemoteDescription = () => {}

  constructor(configuration) {
    this.configuration = configuration
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
    return {
      type: "offer",
      sdp: this.configuration?.iceTransportPolicy === "relay" ? relayOffer : "offer-sdp",
    }
  }
  async setLocalDescription(description) {
    this.localDescription = description
  }
  async setRemoteDescription(description) {
    this.remoteDescription = description
    this.onRemoteDescription()
  }
  addEventListener() {}
  removeEventListener() {}
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

test("uses same-origin signaling and relay-only ICE for a game launch", async (t) => {
  const playerOrigin = "http://localhost:5180"
  globalThis.window = { location: { origin: playerOrigin } }
  t.after(() => delete globalThis.window)
  const instance = "d2379bc0-b611-44aa-8ce0-d1e9bc997b77"
  const stream = `${playerOrigin}/api/games/instances/${instance}/stream`
  const iceServers = [{ urls: "turn:172.30.42.2:3478", username: "test", credential: "test" }]
  const requests = stubFetch(t, (request) => {
    if (request.url === `${stream}/ice`) return Response.json({ iceServers })
    if (request.url === `${stream}/status`) return new Response(null, { status: 410 })
    return agent(request)
  })
  const signal = new AbortController().signal
  const connection = await connectToGame(instance, signal)
  assert.deepEqual(FakePeerConnection.last.configuration, {
    iceServers,
    iceTransportPolicy: "relay",
  })
  assert.deepEqual(
    requests.map((request) => request.url),
    [`${stream}/ice`, `${stream}/offer`],
  )
  assert.equal(await connection.isGameAvailable(signal), false)
  assert.equal(requests[2].init.signal, signal)
})

test("retries with new TURN credentials after a candidate-free offer", async (t) => {
  globalThis.window = { location: { origin: "http://localhost:5180" } }
  t.after(() => delete globalThis.window)
  const peers = []
  const offer = FakePeerConnection.prototype.createOffer
  t.mock.method(FakePeerConnection.prototype, "createOffer", async function () {
    peers.push(this)
    if (peers.length === 1) return { type: "offer", sdp: "v=0\r\n" }
    return offer.call(this)
  })
  let issued = 0
  const requests = stubFetch(t, (request) => {
    if (request.url.endsWith("/ice"))
      return Response.json({
        iceServers: [{ urls: "turn:172.30.42.2:3478", credential: String(++issued) }],
      })
    return agent(request)
  })

  const connection = await connectToGame("game-instance", new AbortController().signal)
  assert.equal(peers[0].closed, true)
  assert.equal(peers[1], connection.peerConnection)
  assert.equal(peers[1].configuration.iceServers[0].credential, "2")
  assert.deepEqual(
    requests.map(({ url }) => new URL(url).pathname.split("/").at(-1)),
    ["ice", "ice", "offer"],
  )
})

test("closes a failed relayed handshake before reconnecting", async (t) => {
  globalThis.window = { location: { origin: "http://localhost:5180" } }
  t.after(() => delete globalThis.window)
  const attempts = []
  const setAnswer = FakePeerConnection.prototype.setRemoteDescription
  t.mock.method(FakePeerConnection.prototype, "setRemoteDescription", async function (description) {
    attempts.push(this)
    if (attempts.length === 1) this.connectionState = "failed"
    return setAnswer.call(this, description)
  })
  const requests = stubFetch(t, (request) =>
    request.url.endsWith("/ice") ? Response.json({ iceServers: [] }) : agent(request),
  )

  const connection = await connectToGame("game-instance", new AbortController().signal)
  assert.equal(attempts[0].closed, true)
  assert.equal(connection.peerConnection, attempts[1])
  assert.deepEqual(
    requests.map(({ method }) => method),
    ["GET", "POST", "DELETE", "GET", "POST"],
  )
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
