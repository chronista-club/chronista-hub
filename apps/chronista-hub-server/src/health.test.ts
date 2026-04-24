import { describe, expect, it } from 'bun:test'
import { createHealthApp } from './health.js'

describe('createHealthApp', () => {
  const info = { name: 'chronista-hub', version: '0.0.1' }

  it('responds 200 on /', async () => {
    const app = createHealthApp(info)
    const res = await app.request('/')
    expect(res.status).toBe(200)
  })

  it('includes status, service, version, timestamp', async () => {
    const app = createHealthApp(info)
    const res = await app.request('/')
    const body = await res.json()
    expect(body.status).toBe('ok')
    expect(body.service).toBe('chronista-hub')
    expect(body.version).toBe('0.0.1')
    expect(typeof body.timestamp).toBe('string')
  })

  it('timestamp is ISO 8601 valid', async () => {
    const app = createHealthApp(info)
    const res = await app.request('/')
    const body = await res.json()
    const d = new Date(body.timestamp)
    expect(d.toString()).not.toBe('Invalid Date')
    expect(body.timestamp).toMatch(/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/)
  })

  it('reflects custom info name/version', async () => {
    const app = createHealthApp({ name: 'other', version: '9.9.9' })
    const res = await app.request('/')
    const body = await res.json()
    expect(body.service).toBe('other')
    expect(body.version).toBe('9.9.9')
  })
})
