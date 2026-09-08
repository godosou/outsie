export type ShortBreakVoiceContext = 'enter' | 'notification' | 'postpone' | 'return' | 'complete'
export type VoiceLine = Readonly<{ title: string; body: string }>

export const SHORT_BREAK_VOICE: Readonly<Record<ShortBreakVoiceContext, readonly VoiceLine[]>> = {
  enter: [
    { title: '电脑没意见，眼睛有。', body: '离开屏幕二十秒。消息我已经替它转达了。' },
    { title: '还在盯？我都替你眨累了。', body: '看向远处，二十秒后再继续逞强。' },
    { title: '工作很急。眼睛表示：它更急。', body: '先暂停二十秒，这封投诉就算撤回。' },
    { title: '屏幕不会跑，休息可能会。', body: '现在看向远处，别让它从日程里溜走。' },
    { title: '温柔版最后通牒来了。', body: '把视线移开二十秒。放心，工作还在那里。' },
    { title: '我数到二十，你看远处。', body: '我们都假装这是你主动决定的。' },
    { title: '你的专注很感人。', body: '你的眼睛不太感动。先看远处。' },
    { title: '收到一份来自眼睛的投诉。', body: '诉求很简单：暂停二十秒。' },
    { title: '休息一下。这个不是建议。', body: '是你的日程安排。二十秒后再忙。' },
    { title: '你当然可以继续盯屏幕。', body: '也可以做个会休息的聪明人。选后者。' },
  ],
  notification: [
    { title: 'Repose 正在看着你的工位。', body: '它发现你又忘了眨眼。' },
    { title: '二十秒休息已送达。', body: '请本人签收，不接受同事代领。' },
    { title: '你和屏幕的会面超时了。', body: '先散会二十秒，回来再谈。' },
    { title: '屏幕说它想静静。', body: '给它，也给自己二十秒。' },
    { title: '本花郑重提醒：', body: '看远处。现在。谢谢配合。' },
    { title: '暂停键不会自己按。', body: '所以我来提醒你，二十秒就好。' },
    { title: '眼睛提交了请假申请。', body: '已批准。申请人请立即离屏。' },
    { title: '别担心，工作还在那里。', body: '我确认过了。先休息二十秒。' },
    { title: '你的休息额度快过期了。', body: '现在使用，不支持转赠。' },
    { title: 'Repose 敲了敲屏幕。', body: '该把视线还给远处了。' },
  ],
  postpone: [
    { title: '行，再给你一分钟。', body: '“马上”最好是真的马上。' },
    { title: '延期申请通过。', body: '我会在六十秒后准时回来。非常准时。' },
    { title: '一分钟。不能再多了。', body: '我已经开始替你计时。' },
    { title: '这次先放过屏幕。', body: '一分钟后，我会更有存在感。' },
    { title: '好，你先收个尾。', body: '尾巴最好别长成下一条龙。' },
    { title: '成交，六十秒。', body: '到点就起身，不许和键盘谈条件。' },
    { title: '我听见你说“马上”了。', body: '证据已保存。一分钟后见。' },
    { title: '可以延迟，不可以消失。', body: '给你一分钟把这件事放下。' },
    { title: '批准，但有条件。', body: '一分钟后把眼睛从屏幕借回来。' },
  ],
  return: [
    { title: '一分钟到了。', body: '你的“马上”，现在归我管。' },
    { title: '我回来了。惊不惊喜？', body: '屏幕可以等，先把目光移开。' },
    { title: '收尾时间结束。', body: '别装没看见，我就在屏幕正中间。' },
    { title: '延期额度已用完。', body: '这次真的休息，二十秒就好。' },
    { title: '刚才那分钟跑得真快。', body: '没关系，我跑得更准时。开始休息。' },
    { title: '你说马上，我记得。', body: '现在就是那个“马上”。' },
    { title: '第二次见面了。', body: '这说明休息比工作更守时。' },
    { title: '我来兑现你的承诺。', body: '离开屏幕，看向远处。' },
    { title: '键盘还想留你。', body: '我替你拒绝了。休息开始。' },
    { title: '时间到，借口下班。', body: '眼睛也下班二十秒。' },
  ],
  complete: [
    { title: '这就对了。', body: '工作可以卷，眼睛不参与。' },
    { title: '不错，眼睛撤回了投诉。', body: '下次主动一点，我会更欣慰。' },
    { title: '二十秒，世界没有停转。', body: '甚至你的工作也还在那里。' },
    { title: '休息完成，批准返岗。', body: '这次我就不盯着你了。暂时。' },
    { title: '看吧，休息并不耽误事。', body: '它只耽误你把自己忘掉。' },
    { title: '任务完成：照顾自己。', body: '这项绩效我给满分。' },
    { title: '眼睛已重新上线。', body: '屏幕可以继续，记得偶尔做人。' },
    { title: '很好，你听劝了。', body: '保持这个难得的好习惯。' },
    { title: '短休息打卡成功。', body: 'Repose 对此表示勉强满意。' },
    { title: '可以继续了。', body: '我会在该出现的时候再次出现。' },
  ],
}

function hash(value: string) {
  let result = 2166136261
  for (let index = 0; index < value.length; index += 1) {
    result ^= value.charCodeAt(index)
    result = Math.imul(result, 16777619)
  }
  return result >>> 0
}

export function getShortBreakVoice(context: ShortBreakVoiceContext, breakId: string): VoiceLine {
  const pool = SHORT_BREAK_VOICE[context]
  return pool[hash(`${context}:${breakId || 'repose'}`) % pool.length]
}
