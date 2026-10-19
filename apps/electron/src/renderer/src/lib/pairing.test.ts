import { describe, expect, it } from 'vitest'
import { pairPicked, pairingItems } from './pairing'

const MEDIA = ['wav', 'mp3', 'mp4', 'mkv']
const TEXT = ['txt', 'srt']

function pair(paths: string[]) {
  return pairPicked(paths, MEDIA, TEXT)
}

describe('pairPicked', () => {
  it('一个媒体 + 一个文稿，名字不同也直接配对', () => {
    const got = pair(['videos/meeting.mp4', 'videos/notes.txt'])
    expect(got.media).toEqual(['videos/meeting.mp4'])
    expect(got.pairs).toEqual([{ mediaIndex: 0, transcript: 'videos/notes.txt' }])
    expect(got.unpairedTranscripts).toEqual([])
  })

  it('顺序无关：先选文稿后选视频也能配上', () => {
    const got = pair(['a.txt', 'a.mp4'])
    expect(got.pairs).toEqual([{ mediaIndex: 0, transcript: 'a.txt' }])
  })

  it('同名匹配不区分大小写和扩展名大小写', () => {
    const got = pair(['A.MP4', 'a.TXT'])
    expect(got.pairs).toEqual([{ mediaIndex: 0, transcript: 'a.TXT' }])
  })

  it('带点的文件名按完整 stem 匹配', () => {
    const got = pair(['a.b.mp4', 'a.b.srt'])
    expect(got.pairs).toEqual([{ mediaIndex: 0, transcript: 'a.b.srt' }])
  })

  it('一份媒体只收第一份同名文稿，第二份报为未配对', () => {
    const got = pair(['a.mp4', 'a.txt', 'a.srt'])
    expect(got.pairs).toEqual([{ mediaIndex: 0, transcript: 'a.txt' }])
    expect(got.unpairedTranscripts).toEqual(['a.srt'])
  })

  it('两个同 stem 的媒体各接一份文稿', () => {
    const got = pair(['a.mp4', 'a.wav', 'a.txt', 'a.srt'])
    expect(got.pairs).toEqual([
      { mediaIndex: 0, transcript: 'a.txt' },
      { mediaIndex: 1, transcript: 'a.srt' }
    ])
  })

  it('只有文稿没有媒体：不建任务，全部报未配对', () => {
    const got = pair(['notes.txt'])
    expect(got.media).toEqual([])
    expect(got.unpairedTranscripts).toEqual(['notes.txt'])
  })

  it('不认识的文件进 skipped，不参与配对', () => {
    const got = pair(['a.mp4', 'a.txt', 'cover.jpg'])
    expect(got.skipped).toEqual(['cover.jpg'])
    expect(got.pairs).toHaveLength(1)
  })

  it('Markdown 不再算文稿，进 skipped', () => {
    const got = pair(['a.mp4', 'a.md', 'a.markdown'])
    expect(got.media).toEqual(['a.mp4'])
    expect(got.pairs).toEqual([])
    expect(got.unpairedTranscripts).toEqual([])
    expect(got.skipped).toEqual(['a.md', 'a.markdown'])
  })

  it('pairingItems 把文稿按媒体下标接好', () => {
    const got = pair(['a.mp4', 'b.mp4', 'a.txt'])
    expect(pairingItems(got)).toEqual([
      { media: 'a.mp4', transcript: 'a.txt' },
      { media: 'b.mp4', transcript: undefined }
    ])
  })
})
