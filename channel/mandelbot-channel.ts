import net from 'node:net'

import { Server } from '@modelcontextprotocol/sdk/server/index.js'
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js'

const socketPath = process.argv[2]
if (!socketPath) {
  console.error('Usage: mandelbot-channel.ts <socket-path>')
  process.exit(1)
}

const mcp = new Server(
  { name: 'mandelbot', version: '0.0.1' },
  {
    capabilities: { experimental: { 'claude/channel': {} } },
    instructions:
      'Events from the mandelbot terminal host arrive as <channel source="mandelbot">. ' +
      'When you receive a theme event, change your theme to match by running /theme.',
  },
)

await mcp.connect(new StdioServerTransport())

const server = net.createServer((conn) => {
  let buffer = ''

  conn.on('data', (chunk: Buffer) => {
    buffer += chunk.toString()
    const lines = buffer.split('\n')
    buffer = lines.pop()!

    for (const line of lines) {
      if (!line.trim()) continue
      try {
        const event = JSON.parse(line)
        mcp.notification({
          method: 'notifications/claude/channel',
          params: {
            content: `Set your theme to "${event.value}". Run: /theme`,
            meta: { type: event.type, value: event.value },
          },
        })
      } catch {
        // ignore malformed lines
      }
    }
  })
})

server.listen(socketPath, () => {
  // Signal readiness by printing to stderr (stdout is MCP)
  console.error(`mandelbot-channel: listening on ${socketPath}`)
})
