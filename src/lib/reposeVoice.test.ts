import assert from 'node:assert/strict'
import test from 'node:test'
import { SHORT_BREAK_VOICE, getShortBreakVoice, type ShortBreakVoiceContext } from './reposeVoice.ts'

const contexts: ShortBreakVoiceContext[] = ['enter', 'notification', 'postpone', 'return', 'complete']

test('every short-break moment has a substantial original copy pool', () => {
  for (const context of contexts) {
    assert.ok(SHORT_BREAK_VOICE[context].length >= 8, `${context} needs at least eight variants`)
    assert.equal(new Set(SHORT_BREAK_VOICE[context].map(line => `${line.title}\n${line.body}`)).size, SHORT_BREAK_VOICE[context].length)
  }
})

test('a break id selects stable copy while different ids rotate the pool', () => {
  for (const context of contexts) {
    assert.deepEqual(getShortBreakVoice(context, 'break-42'), getShortBreakVoice(context, 'break-42'))
    const selected = new Set(Array.from({ length: 40 }, (_, index) => getShortBreakVoice(context, `break-${index}`).title))
    assert.ok(selected.size >= 5, `${context} should visibly rotate`) 
  }
})

test('the playful voice avoids threats, shame and invented health claims', () => {
  const forbidden = /瞎|失明|废物|没用|杀|死|完蛋|报废|惩罚|活该|猝死|病变|不休息.*后果/
  for (const context of contexts) {
    for (const line of SHORT_BREAK_VOICE[context]) {
      assert.ok(line.title.length > 2 && line.title.length <= 30)
      assert.ok(line.body.length > 2 && line.body.length <= 50)
      assert.doesNotMatch(`${line.title}${line.body}`, forbidden)
    }
  }
})
