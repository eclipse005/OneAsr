export type UiLang = 'zh' | 'en'

export const zh = {
  appName: 'OneAsr',
  add: '添加',
  startAll: '全部开始',
  clear: '清空',
  cancel: '取消',
  cancelQueued: '取消排队',
  retranscribe: '重新转录',
  applyToAll: '应用到全部行',
  settings: '设置',
  stats: '统计',
  guide: '指南',
  hotwords: '热词',
  save: '保存',
  reset: '重置',
  unsaved: '有未保存更改',
  saved: '已保存',
  openSettings: '打开设置',
  emptyTitle: '拖放音视频到此处',
  emptyTitleNotReady: '先把模型准备好',
  emptyHintNotReady: '状态栏显示模型未就绪 —— 在「设置 → 模型」里指到权重目录，或把权重放进 models/ 再刷新',
  emptyHint: '或点击「添加」选择文件 · 音视频，或与媒体同名的 txt / srt 文稿',
  dropActive: '松开以添加文件',
  modelsReady: '模型就绪',
  modelsNotReady: '模型未就绪',
  asr: '识别',
  aligner: '对齐',
  demucs: '人声分离',
  sensevoice: '粗转',
  start: '开始',
  openFolder: '打开目录',
  openOutput: '打开输出',
  remove: '移除',
  retry: '重试',
  copyError: '复制错误',
  noOutputYet: '尚未生成输出',
  copied: '已复制',
  timing: '耗时',
  timingTitle: '处理耗时',
  probing: '探测中…',
  hotwordNone: '无热词',
  noTranscript: '无文稿',
  queued: '排队',
  pending: '待处理',
  blockNothing: '没有可开始的任务',
  // 阶段名：CLI 的事件只保证 key 稳定（label 跟它的进程语言走），界面文案归壳
  stageConverting: '转码音频',
  stageSeparating: '人声分离',
  stagePlanning: '分段规划',
  stageRoughTranscribing: '热词提取',
  stageLoadingAsr: '加载识别模型',
  stageTranscribing: '转写',
  stageLoadingAligner: '加载对齐模型',
  stageAligning: '打轴',
  stageExporting: '导出字幕',
  clearedKeptRunning: '已清空；正在跑的那条继续',
  progress: '进度',
  running: '进行中',
  done: '完成',
  error: '失败',
  cancelled: '已取消',
  uiLanguage: '界面语言',
  // 语言名用各自的母语写法：中文界面也显示「中文 / English」，不翻译。
  uiLanguageZh: '中文',
  uiLanguageEn: 'English',
  recognitionLanguage: '识别语言',
  subtitleLength: '字幕长度',
  chunkDuration: '分段时长（秒）',
  outputs: '输出格式',
  outputKeepOne: '至少留一种格式',
  outputNoneOn: '三种都关掉时仍然会输出 SRT',
  chineseScript: '中文字形',
  subtitlePolish: '成稿字幕美化',
  outputPath: '输出目录',
  saveNextToSource: '保存到源文件旁',
  asrSize: 'ASR 尺寸',
  quantization: '量化',
  asrPath: 'ASR 路径',
  alignerKind: '对齐引擎',
  alignerPath: '对齐路径',
  vocalSep: '人声分离',
  vocalSepDefault: '新任务默认启用',
  vocalSepDefaultShort: '默认启用',
  demucsPath: 'Demucs 路径',
  computeBackend: '计算后端',
  sounds: '提示音',
  closeToTray: '关闭时隐藏到托盘',
  diagnostics: '诊断日志',
  openLog: '打开日志',
  clearLog: '清理',
  logEmpty: '无日志',
  system: '跟随系统',
  short: '短',
  standard: '标准',
  loose: '宽松',
  simplified: '简体',
  traditional: '繁体',
  original: '原文',
  auto: '自动',
  gpu: 'GPU',
  cpu: 'CPU',
  fp16: 'FP16',
  int8: 'INT8',
  ctc: 'CTC',
  qwen: 'Qwen',
  browse: '浏览…',
  openModels: '打开目录',
  refreshModelsStatus: '刷新模型状态',
  downloadSection: '模型',
  allModelsReady: '四类权重都在 models/ 下就绪了',
  download: '下载',
  redownload: '重新下载',
  downloading: '下载中',
  verifying: '校验中',
  downloadDone: '下载完成',
  downloadFailed: '下载失败',
  cancelDownload: '取消下载',
  ready: '就绪',
  missing: '缺失',
  statsEmpty: '还没有运行记录 — 完成一次转写后这里会亮起来',
  minimize: '最小化',
  maximize: '最大化',
  statsTitle: '运行统计',
  totalRuns: '总次数',
  success: '成功',
  failures: '失败',
  savedTime: '节省时长',
  avgSpeed: '平均倍速',
  cues: '字幕条数',
  mediaProcessed: '处理媒体',
  heatTitle: '近 90 日活跃',
  langTitle: '语言分布',
  recent: '本次会话',
  lastDuration: '最近耗时',
  guideTitle: '使用说明',
  guideReadyLead: '先看窗口底部',
  guideReadyHint: '是这个就可以开始。若显示未就绪，点它去准备模型。',
  guideDropLead: '把文件放进来',
  guideDropZone: '拖到窗口',
  guideDropHint: '也可以点添加。同名的 txt / srt 会一起挂成文稿。',
  guideRowLead: '然后看这一行',
  guideSampleName: '示例.mp4',
  guideSampleLang: '中文',
  guideLangHint: '语言和热词，只影响这一行。',
  guideSepHint: '录音吵就打开。需要人声分离模型。',
  guideTranscriptHint: '挂上后变色。改过正文会变成警告色，打轴仍用右边的开始。',
  guideStartHint: '开始这一行。全部开始只跑待处理和失败的。',
  guideOutputHint: '完成后打开字幕。',
  guideRemoveHint: '从列表移除。',
  guideTabUse: '使用',
  guideUiLanguage: '界面用中文还是英文。选跟随系统时看操作系统的语言。改完文案马上换，要点保存才会记住。',
  guideLanguage: '以后新加的任务用这个语言。已经在列表里的，用每一行自己的语言。打轴也把这一行的语言交给对齐。',
  guideSubtitleLength: '自动断句的松紧。短句更好读，宽松少换行。你自己准备的文稿按原来的行来，不受这个影响。',
  guideChunk: '长音频切成多长再识别。短一些更稳，长一些少一次衔接。可选 30、60、90、120、180 秒。',
  guideOutputs: 'SRT 是带时间的字幕，TXT 是纯文本，ASS 是同样的字幕再逐字上色。至少留一种。只开 TXT 时不能打轴，因为打轴要写出带时间的字幕。',
  guideScript: '中文和粤语可以转成简体或繁体，也可以保持模型写出的原文。其他语言忽略这项。不会改写你自己提供的文稿。',
  guidePolish: '写出 SRT 和 ASS 之前，整理标点和全角半角，并在汉字与英文、数字之间加空格。不重新断句，不改 TXT。纯英文的句子保持原样。',
  guideSaveNext: '打开后，字幕写在媒体文件旁边。那个目录写不了时，改用下面的输出目录。',
  guideOutputDir: '不保存到源文件旁时，字幕写到这个目录。',
  guideAsr: '0.6B 和 1.7B 是两套识别权重，一次只用选中的那套。每个尺寸记住自己的目录。',
  guideQuant: 'FP16 是常规权重，INT8 是更小的那套。它和尺寸一起决定加载哪一个识别模型。',
  guideAsrPath: '当前这套识别权重所在的目录。窗口底部的就绪状态看的就是这一套。',
  guideAligner: '对齐默认用 CTC。Qwen 留给旧的模型目录。转写后的时间轴和文稿打轴都走这个引擎。',
  guideVocal: '这是新任务的默认。打开后，新加的行默认做人声分离；已经在列表里的行用自己的开关。吵的录音会先分出人声再识别，需要人声分离的权重。',
  guideDemucs: '人声分离权重所在的目录。',
  guideSense: '用了热词时，会先用这个模型听一遍，再挑出要提示给识别的词。这一行没有热词时不会加载它。',
  guideDownload: '这里只列出还没就绪的模型。下载时，大小已经对得上的文件会跳过，缺的或大小不对的才会重新获取。',
  guideModelPathsTitle: '路径与保存',
  guideModelPaths: '每个模型各用各的路径：ASR 尺寸、对齐器、Demucs、SenseVoice 互不覆盖。改动点保存后才用于转写；下载会落到对应行当前显示的路径。',
  guideHwInputTitle: '怎么添加',
  guideHwInput:
    '一行一个，或用中文、英文逗号分隔（顿号不算），点「添加」或按回车。只写正确写法，例如：窦神归来，OneAsr，Transformer。重复的词会自动合并（英文不区分大小写），纯标点会被忽略。',
  guideHwGroups: '热词按组保存。一组里放希望识别时更不容易听错的专名、缩写和生僻词。',
  guideHwDefault:
    '打开自动启用后，新任务会带上当前的默认组；默认组还没有词、或语言不是中文/英文时，新任务仍是无热词。关掉时新任务不带热词，但记住哪一组是默认。已经在列表里的行不变。',
  guideHwRow:
    '热词只影响这一行的转写，而且只对中文和英文；其它语言这一栏固定为「无热词」，不能选。点行里的「热词」chip 可以换组或选「无热词」；挂了文稿去做打轴时不用热词。',
  guideBackend: '自动会按独显、核显、CPU 的顺序试。选 GPU 时没有可用显卡就直接失败。选 CPU 则不用显卡。',
  guideSounds: '这个开关会保存。任务完成时的提示音还没接到界面上。',
  guideTray: '开着时，点窗口关闭会缩到托盘，任务继续跑。托盘菜单里的退出才会真正关掉。关掉时，点窗口关闭就是退出。',
  guideLog: '报错记在程序目录的日志里。设置里能看到当前这份和上一份轮转加在一起的大小。打开日志可以把报错原样发给别人。清理会删掉这两份，下一笔报错会重新写上这次运行的信息。',
  guideTabTranscript: '文稿',
  guideTranscriptPair:
    '拖进来的 .txt / .srt 会按文件名跟媒体配对，挂到同一行；没配上同名的会单独点出来，不会被悄悄丢掉。也可以点行里的「文稿」chip 自己选一个。',
  guideTranscriptPanel:
    '点开文稿 chip 打开面板：左边是摘要（行数、字数、语速），右边是正文。正文左边的号就是字幕号 —— 一条字幕太长会在屏幕上换行显示，但号只出现一次，所以一眼能看出「这是一条长字幕」还是「两条字幕」。',
  guideTranscriptEdit:
    '面板里的修改只改应用内的副本，源文件永远不会被写入。没保存就关掉面板会问你要不要丢改动。',
  guideTranscriptSplit:
    '智能断句把过长的行拆成短行，拆完仍然只留在应用里，源文件一个字都不动。',
  guideTranscriptRealign:
    '保存只更新这一行的文稿。打轴用那一行外面的开始按钮 —— 文稿匹配跳过识别，直接对齐，产出的字幕跟转写那条路写在一起。改过正文之后，这一行会提示需要重新打轴。',
  guideTranscriptTimed:
    '文稿匹配至少要开 SRT 或 ASS：打轴要写出带时间的字幕，只开 TXT 时这一行不能打轴。',
  sectionGeneral: '通用',
  sectionOutput: '输出',
  sectionModels: '模型',
  sectionHotwords: '热词',
  sectionCompute: '系统',
  close: '关闭',
  pickModel: '选择模型目录',
  hotwordTitle: '热词',
  groupName: '组名',
  groupNamePh: '组名',
  words: '热词',
  wordsPh: '粘贴多词：换行或逗号分隔…',
  wordsHint: '点胶囊上的 × 可删除。',
  batchPaste: '粘贴批量添加',
  batchPasteHide: '收起批量添加',
  batchApply: '合并进本组',
  emptyChips: '暂无热词',
  emptyChipsExample: '例：窦神归来，OneAsr，Transformer',
  addGroup: '新建组',
  deleteGroup: '删除本组',
  defaultGroup: '新任务默认启用',
  defaultGroupShort: '自动启用',
  setDefault: '设为默认',
  isDefault: '默认',
  wordCount: '词',
  groups: '热词组',
  addWord: '添加',
  addWordPh: '一词回车，或粘贴多词（逗号分隔）后添加',
  discard: '放弃更改',
  params: '召回参数',
  topK: 'Top-K',
  threshold: '阈值',
  emptyGroup: '未命名',
  confirmDiscard: '有未保存更改，确定关闭？',
  confirmDeleteGroup: '删除这个词组？',
  discardChanges: '放弃更改',
  restoreDefaults: '恢复默认',
  restored: '已恢复默认设置',
  removeWord: '移除',
  sensevoicePath: '粗转模型路径',
  dialogFailed: '打开文件对话框失败',
  noFiles: '未选择文件',
  transcript: '文稿',
  transcriptAdd: '+ 文稿',
  transcriptLines: '行',
  transcriptOpen: '查看 / 编辑文稿',
  transcriptLocked: '排队或运行中不可修改文稿',
  transcriptRequiresTimedOutput: '文稿匹配需要至少启用 SRT 或 ASS 输出',
  transcriptStart: '开始打轴',
  transcriptRealign: '重新打轴',
  transcriptRealignChip: '待重新打轴',
  transcriptRetryAlign: '重试打轴',
  transcriptPanelTitle: '文稿匹配',
  transcriptSource: '源文件',
  transcriptDroppedTimecodes: '已忽略原 SRT 时间轴，将重新打轴',
  transcriptNeedsRealign: '文稿已修改，需要重新打轴',
  transcriptSmartSplit: '智能断句',
  transcriptReread: '重新读取',
  transcriptRemove: '移除文稿',
  transcriptSave: '保存',
  hotwordIgnoredOnAlign: '打轴不使用热词',
  hotwordUnsupportedLanguage: '热词只用于中文和英文',
  transcriptSaved: '文稿已保存',
  transcriptSplitDone: '已智能断句',
  transcriptRereadDone: '已重新读取源文件',
  transcriptRemoved: '已移除文稿',
  transcriptReadFailed: '读不到文稿',
  transcriptSaveFailed: '保存文稿失败',
  transcriptSplitFailed: '智能断句失败',
  transcriptNoSource: '这份文稿没有源文件',
  transcriptUnsaved: '未保存',
  transcriptDensityStale: '保存后更新语速提示',
  transcriptConfirmDiscard: '有未保存修改，确定关闭？',
  transcriptDropHint: '把 .txt / .srt 拖到这一行，可以挂成文稿',
  transcriptNoMedia: '没有可添加的媒体文件',
  modelsLoading: '正在读取模型状态…',
  backend: '后端',
  modelCatalog: '模型目录',
  modelInstalled: '已安装',
  modelNotInstalled: '未安装',
  modelSelected: '当前选用',
  downloadAndSelect: '下载并选用',
  downloadSelect: '下载',
  advancedPaths: '高级 · 路径',
  cliOutdated: '引擎 CLI 过旧，请更新 oneasr-cli（缺少 status / transcript）',
  cliOutdatedShort: 'CLI 过旧，请更新引擎',
  asrPurpose: '识别',
  alignPurpose: '对齐',
  demucsPurpose: '人声分离',
  sensePurpose: '粗转',
  modelRetry: '重试',
  sparklineAria: '近 30 天处理时长趋势',
  // —— 交互引导（coach-mark tour）——
  tourInteractive: '交互引导',
  tourManual: '使用说明',
  tourWelcomeTitle: '欢迎使用 OneAsr',
  tourWelcomeBody: '花一两分钟点一遍真实控件，或先跳过，以后可从工具栏「指南」再开。',
  tourStart: '开始引导',
  tourSkip: '跳过',
  tourNext: '下一步',
  tourFinish: '完成',
  tourClickHint: '请点击高亮的控件继续',
  tourClickOrNextHint: '请点击高亮的控件，或点「跳过此步」',
  tourSkipStep: '跳过此步',
  tourSkipWait: '跳过下载等待',
  tourStatusTitle: '模型状态',
  tourStatusBody: '窗口底部显示模型是否就绪。未就绪时点这里进入「设置 → 模型」；也可点「跳过此步」。',
  tourCliTitle: '引擎 CLI 过旧',
  tourCliBody: '状态栏橙色提示表示引擎 CLI 需要更新，不是模型未就绪。点「设置」打开「设置 → 模型」查看详情；也可跳过此步。',
  tourModelsTitle: '模型页',
  tourModelsBody: '左侧可切换模型 / 热词等页。这里选择识别尺寸、对齐引擎并指定权重目录；底部状态栏反映的就是这一套。',
  tourDownloadTitle: '下载权重',
  tourDownloadBody: '缺权重时点这个下载按钮。下载不会打断引导；也可先跳过，稍后再下。',
  tourHotwordsTabTitle: '热词标签',
  tourHotwordsTabBody: '点「热词」打开词库页，给专名、缩写加提示。',
  tourHotwordsTitle: '热词词库',
  tourHotwordsBody: '按组管理热词。行内还可以单独选组；空默认组不会绑到新任务。',
  tourOutputTabTitle: '输出标签',
  tourOutputTabBody: '点「输出」查看字幕格式与保存位置。',
  tourOutputTitle: '输出设置',
  tourOutputBody: 'SRT / TXT / ASS 至少留一种。文稿打轴需要带时间的格式（SRT 或 ASS）。',
  tourSettingsCloseTitle: '关闭设置',
  tourSettingsCloseBody: '改完记得保存。点关闭（或「跳过此步」）回到主界面继续。',
  tourAddTitle: '添加文件',
  tourAddBody: '点「添加」或把音视频拖进窗口。同名的 txt / srt 会挂成文稿。不想现在添加可点「下一步」跳过。',
  tourDropzoneTitle: '拖放区',
  tourDropzoneBody: '列表为空且模型已就绪时，这里就是落点。把音视频拖进来即可。',
  tourEmptySettingsTitle: '先准备模型',
  tourEmptySettingsBody: '模型未就绪时这里不能投文件。点「打开设置」去准备权重，或跳过此步。',
  tourTaskLangTitle: '识别语言',
  tourTaskLangBody: '这一行自己的语言。只影响本行，可「应用到全部行」。',
  tourTaskHotwordsTitle: '行内热词',
  tourTaskHotwordsBody: '给这一行选词库。中英文以外的语言不支持热词。',
  tourTaskSepTitle: '人声分离',
  tourTaskSepBody: '录音吵就打开。需要人声分离模型；空心关、实心开。',
  tourTaskTranscriptTitle: '文稿',
  tourTaskTranscriptBody: '挂上文稿后变色。也可把 txt / srt 拖到这一行。打轴仍用右边的开始。',
  tourTaskStartTitle: '开始',
  tourTaskStartBody: '开始这一行。全部开始只跑待处理和失败的。引导里不会替你按下，以免误开转写。',
  tourTaskOutputTitle: '打开输出',
  tourTaskOutputBody: '完成后点这里打开字幕所在目录。',
  tourStatsTitle: '统计面板',
  tourStatsBody: '这是统计面板：处理量与近期情况。看完点「下一步」继续。',
  tourGuideReopenTitle: '再次打开',
  tourGuideReopenBody: '以后从「指南」可重开交互引导，或看静态「使用说明」。',
  tourDoneTitle: '可以开始了',
  tourDoneBody: '引导结束。Esc 也可随时退出引导；不会取消正在跑的任务。',
} as const

type Dict = { [K in keyof typeof zh]: string }

export const en: Dict = {
  appName: 'OneAsr',
  add: 'Add',
  startAll: 'Start all',
  clear: 'Clear',
  cancel: 'Cancel',
  cancelQueued: 'Cancel queue',
  retranscribe: 'Transcribe again',
  applyToAll: 'Apply to all rows',
  settings: 'Settings',
  stats: 'Stats',
  guide: 'Guide',
  hotwords: 'Hotwords',
  save: 'Save',
  reset: 'Reset',
  unsaved: 'Unsaved changes',
  saved: 'Saved',
  openSettings: 'Open settings',
  emptyTitle: 'Drop media here',
  emptyTitleNotReady: 'Set up models first',
  emptyHintNotReady: 'Models are not ready — point to the weights in Settings → Models, or drop them under models/ and refresh',
  emptyHint: 'Or click Add · media, plus a matching txt / srt transcript',
  dropActive: 'Release to add',
  modelsReady: 'Models ready',
  modelsNotReady: 'Models not ready',
  asr: 'ASR',
  aligner: 'Aligner',
  demucs: 'Vocals',
  sensevoice: 'SenseVoice',
  start: 'Start',
  openFolder: 'Open folder',
  openOutput: 'Open output',
  remove: 'Remove',
  retry: 'Retry',
  copyError: 'Copy error',
  noOutputYet: 'No output yet',
  copied: 'Copied',
  timing: 'Timing',
  timingTitle: 'Processing time',
  probing: 'probing…',
  hotwordNone: 'No hotwords',
  noTranscript: 'No transcript',
  queued: 'Queued',
  pending: 'Pending',
  blockNothing: 'Nothing to start',
  stageConverting: 'Converting audio',
  stageSeparating: 'Separating vocals',
  stagePlanning: 'Planning segments',
  stageRoughTranscribing: 'Extracting hotwords',
  stageLoadingAsr: 'Loading ASR model',
  stageTranscribing: 'Transcribing',
  stageLoadingAligner: 'Loading aligner model',
  stageAligning: 'Aligning',
  stageExporting: 'Exporting subtitles',
  clearedKeptRunning: 'Cleared — the running task keeps going',
  progress: 'Progress',
  running: 'Running',
  done: 'Done',
  error: 'Error',
  cancelled: 'Cancelled',
  uiLanguage: 'UI language',
  uiLanguageZh: '中文',
  uiLanguageEn: 'English',
  recognitionLanguage: 'Recognition language',
  subtitleLength: 'Subtitle length',
  chunkDuration: 'Chunk duration (s)',
  outputs: 'Outputs',
  outputKeepOne: 'Keep at least one format',
  outputNoneOn: 'SRT is still written when all three are off',
  chineseScript: 'Chinese script',
  subtitlePolish: 'Subtitle polish',
  outputPath: 'Output path',
  saveNextToSource: 'Save next to source',
  asrSize: 'ASR size',
  quantization: 'Quantization',
  asrPath: 'ASR path',
  alignerKind: 'Aligner engine',
  alignerPath: 'Aligner path',
  vocalSep: 'Vocals',
  vocalSepDefault: 'Default for new tasks',
  vocalSepDefaultShort: 'Default on',
  demucsPath: 'Demucs path',
  computeBackend: 'Compute backend',
  sounds: 'Sounds',
  closeToTray: 'Hide to tray on close',
  diagnostics: 'Diagnostics log',
  openLog: 'Open log',
  clearLog: 'Clear',
  logEmpty: 'No log',
  system: 'System',
  short: 'Short',
  standard: 'Standard',
  loose: 'Loose',
  simplified: 'Simplified',
  traditional: 'Traditional',
  original: 'Original',
  auto: 'Auto',
  gpu: 'GPU',
  cpu: 'CPU',
  fp16: 'FP16',
  int8: 'INT8',
  ctc: 'CTC',
  qwen: 'Qwen',
  browse: 'Browse…',
  openModels: 'Open folder',
  refreshModelsStatus: 'Refresh model status',
  downloadSection: 'Models',
  allModelsReady: 'All four weights are present under models/',
  download: 'Download',
  redownload: 'Re-download',
  downloading: 'Downloading',
  verifying: 'Verifying',
  downloadDone: 'Download complete',
  downloadFailed: 'Download failed',
  cancelDownload: 'Cancel download',
  ready: 'Ready',
  missing: 'Missing',
  statsEmpty: 'No runs yet — finish a transcription to light this up',
  minimize: 'Minimize',
  maximize: 'Maximize',
  statsTitle: 'Run stats',
  totalRuns: 'Total',
  success: 'Success',
  failures: 'Failures',
  savedTime: 'Time saved',
  avgSpeed: 'Avg realtime',
  cues: 'Cues',
  mediaProcessed: 'Media processed',
  heatTitle: 'Last 90 days',
  langTitle: 'Languages',
  recent: 'This session',
  lastDuration: 'Last duration',
  guideTitle: 'Manual',
  guideReadyLead: 'Look at the bottom first',
  guideReadyHint: 'This means you can start. If it says not ready, click it to set up models.',
  guideDropLead: 'Bring a file in',
  guideDropZone: 'Drop it here',
  guideDropHint: 'Or click Add. A txt / srt with the same name is attached as the transcript.',
  guideRowLead: 'Then use the row',
  guideSampleName: 'sample.mp4',
  guideSampleLang: 'English',
  guideLangHint: 'Language and hotwords apply to this row only.',
  guideSepHint: 'Turn on for noisy audio. Needs the separation model.',
  guideTranscriptHint: 'Turns accent when attached. After an edit it turns amber. Align with the start button.',
  guideStartHint: 'Starts this row. Start all runs pending and failed rows only.',
  guideOutputHint: 'Opens the subtitles when the row finishes.',
  guideRemoveHint: 'Removes the row.',
  guideTabUse: 'Using',
  guideUiLanguage: 'Which language the interface uses. System follows the operating system. The words switch at once; Save remembers the choice.',
  guideLanguage: 'Language for tasks you add later. Rows already in the list keep their own. Alignment uses that row’s language too.',
  guideSubtitleLength: 'How tightly automatic captions are split. Short is easier to read; loose breaks the line less often. A transcript you supply keeps its own lines.',
  guideChunk: 'How long each piece of a long recording is before recognition. Shorter is steadier; longer means fewer joins. The choices are 30, 60, 90, 120, and 180 seconds.',
  guideOutputs: 'SRT is timed subtitles, TXT is plain text, and ASS is those cues with a colour sweep. Keep at least one. Alignment needs SRT or ASS, so a transcript cannot be aligned when only TXT is on.',
  guideScript: 'Chinese and Cantonese can be written as simplified or traditional, or left as the model wrote them. Other languages ignore this. It does not rewrite a transcript you supplied.',
  guidePolish: 'Before SRT and ASS are written, punctuation and full-width characters are tidied, and a space is added between Han characters and Latin words or digits. Lines are not rebroken, TXT is left alone, and an English-only line stays as it is.',
  guideSaveNext: 'When on, subtitles are written next to the media file. If that folder cannot be written, the output folder below is used.',
  guideOutputDir: 'Where subtitles go when they are not saved next to the source.',
  guideAsr: '0.6B and 1.7B are two recognition weights. Only the selected one is used, and each size remembers its own folder.',
  guideQuant: 'FP16 is the standard weight. INT8 is the smaller one. Together with the size, this picks which recognition model is loaded.',
  guideAsrPath: 'Folder for the selected recognition weights. The status at the bottom of the window is whether this set is ready.',
  guideAligner: 'CTC is the default aligner. Qwen is kept for older model folders. Transcription timestamps and transcript alignment both use this engine.',
  guideVocal: 'This is the default for tasks you add later. Rows already in the list keep their own switch. Noisy recordings can be separated before recognition, which needs the vocal-separation weights.',
  guideDemucs: 'Folder for the vocal-separation weights.',
  guideSense: 'When a row uses hotwords, this model listens first and picks which terms to hint to recognition. It is not loaded when the row has no hotwords.',
  guideDownload: 'Only models that are not ready are listed. Files that are already the right size are skipped. Missing or wrong-size files are fetched.',
  guideModelPathsTitle: 'Paths and saving',
  guideModelPaths: 'Each model keeps its own folder: ASR sizes, aligner, Demucs, and SenseVoice do not overwrite one another. Changes apply to transcription after Save; downloads go to the folder shown on that row.',
  guideHwInputTitle: 'How to add',
  guideHwInput:
    'One per line, or separated by commas (Chinese or English; the Chinese enumeration comma (、) does not count). Click Add or press Enter. Write only the correct spelling, for example: Quilter, OneAsr, Transformer. Duplicates are merged (English is case-insensitive) and punctuation-only entries are ignored.',
  guideHwGroups: 'Hotwords are saved in groups. A group holds the names, abbreviations, and unusual words you want the recogniser to catch.',
  guideHwDefault:
    'With Auto-enable on, new tasks take the default group; if the default group has no words, or the language is not Chinese or English, new tasks still start with no hotwords. Turning it off leaves new tasks without hotwords and still remembers which group was the default. Rows already in the list stay as they are.',
  guideHwRow:
    'A row’s hotwords affect only that transcription, and only for Chinese or English; other languages show No hotwords and cannot pick a group. Click the Hotwords chip on the row to change the group or choose No hotwords. Aligning a transcript does not use hotwords.',
  guideBackend: 'Auto tries a discrete GPU, then an integrated GPU, then CPU. GPU fails outright when no adapter is available. CPU does not use a graphics adapter.',
  guideSounds: 'The switch is saved. A sound when a task finishes is not wired up yet.',
  guideTray: 'When on, closing the window hides it to the tray and tasks keep running. Quit from the tray menu to exit. When off, closing the window quits.',
  guideLog: 'Errors are written to a log in the app folder. Settings shows the size of the current file plus the previous rotation. Open the log to copy an error as-is. Clear deletes both, and the next error starts a new file with this run’s details.',
  guideTabTranscript: 'Transcript',
  guideTranscriptPair:
    'A .txt / .srt you drop is paired with the media by file name and attached to the same row. One without a match is listed on its own instead of being dropped silently. You can also pick one with the transcript chip in the row.',
  guideTranscriptPanel:
    'Open the transcript chip for the panel: summary on the left (lines, characters, speech rate), the text on the right. The number on the left is the subtitle number — a subtitle too long for the window wraps on screen but keeps its single number, so you can tell one long cue from two cues at a glance.',
  guideTranscriptEdit:
    'Edits only change the copy inside the app; the source file is never written. Closing the panel with unsaved changes asks whether to discard them.',
  guideTranscriptSplit:
    'Smart split breaks long lines into short ones. The result still stays inside the app — not a single character of the source file is touched.',
  guideTranscriptRealign:
    'Saving only updates this row’s transcript. Aligning is started with the row’s own start button: transcript matching skips recognition and aligns directly, writing its subtitle alongside the ones from transcription. Once the text changed, the row says it needs a new alignment.',
  guideTranscriptTimed:
    'Transcript matching needs SRT or ASS: alignment writes a timestamped subtitle, so a TXT-only setup cannot align.',
  sectionGeneral: 'General',
  sectionOutput: 'Output',
  sectionModels: 'Models',
  sectionHotwords: 'Hotwords',
  sectionCompute: 'System',
  close: 'Close',
  pickModel: 'Choose model folder',
  hotwordTitle: 'Hotwords',
  groupName: 'Group name',
  groupNamePh: 'Name',
  words: 'Terms',
  wordsPh: 'Paste many terms: newlines or commas…',
  wordsHint: 'Click × on a chip to remove.',
  batchPaste: 'Paste batch',
  batchPasteHide: 'Hide batch paste',
  batchApply: 'Merge into group',
  emptyChips: 'No hotwords yet',
  emptyChipsExample: 'e.g. Quilter, OneAsr, Transformer',
  addGroup: 'New group',
  deleteGroup: 'Delete group',
  defaultGroup: 'Default for new tasks',
  defaultGroupShort: 'Auto-enable',
  setDefault: 'Set default',
  isDefault: 'Default',
  wordCount: 'words',
  groups: 'Hotword groups',
  addWord: 'Add',
  addWordPh: 'Hotword + Enter, or paste many (comma-separated) then Add',
  discard: 'Discard',
  params: 'Recall params',
  topK: 'Top-K',
  threshold: 'Threshold',
  emptyGroup: 'Untitled',
  confirmDiscard: 'Discard unsaved changes?',
  confirmDeleteGroup: 'Delete this group?',
  discardChanges: 'Discard changes',
  restoreDefaults: 'Restore defaults',
  restored: 'Defaults restored',
  removeWord: 'Remove',
  sensevoicePath: 'SenseVoice path',
  dialogFailed: 'File dialog failed',
  noFiles: 'No files selected',
  transcript: 'Transcript',
  transcriptAdd: '+ Text',
  transcriptLines: 'lines',
  transcriptOpen: 'View / edit transcript',
  transcriptLocked: 'Transcript is locked while queued or running',
  transcriptRequiresTimedOutput: 'Transcript matching needs SRT or ASS output',
  transcriptStart: 'Start aligning',
  transcriptRealign: 'Realign',
  transcriptRealignChip: 'Needs realign',
  transcriptRetryAlign: 'Retry aligning',
  transcriptPanelTitle: 'Transcript match',
  transcriptSource: 'Source',
  transcriptDroppedTimecodes: 'Original SRT timings ignored; the transcript will be realigned',
  transcriptNeedsRealign: 'Transcript changed; realign to update the subtitles',
  transcriptSmartSplit: 'Smart split',
  transcriptReread: 'Re-read',
  transcriptRemove: 'Remove transcript',
  transcriptSave: 'Save',
  hotwordIgnoredOnAlign: 'Alignment does not use hotwords',
  hotwordUnsupportedLanguage: 'Hotwords are only available for Chinese and English',
  transcriptSaved: 'Transcript saved',
  transcriptSplitDone: 'Smart split applied',
  transcriptRereadDone: 'Reloaded from source',
  transcriptRemoved: 'Transcript removed',
  transcriptReadFailed: 'Could not read transcript',
  transcriptSaveFailed: 'Could not save transcript',
  transcriptSplitFailed: 'Smart split failed',
  transcriptNoSource: 'This transcript has no source file',
  transcriptUnsaved: 'Unsaved',
  transcriptDensityStale: 'Save to refresh the rate check',
  transcriptConfirmDiscard: 'Discard unsaved transcript changes?',
  transcriptDropHint: 'Drop a .txt / .srt on this row to attach it as the transcript',
  transcriptNoMedia: 'No media files to add',
  modelsLoading: 'Loading model status…',
  backend: 'Backend',
  modelCatalog: 'Model catalog',
  modelInstalled: 'Installed',
  modelNotInstalled: 'Not installed',
  modelSelected: 'Selected',
  downloadAndSelect: 'Download & select',
  downloadSelect: 'Download',
  advancedPaths: 'Advanced · paths',
  cliOutdated: 'Engine CLI is outdated — update oneasr-cli (missing status / transcript)',
  cliOutdatedShort: 'CLI outdated — update engine',
  asrPurpose: 'ASR',
  alignPurpose: 'Aligner',
  demucsPurpose: 'Vocals',
  sensePurpose: 'Hotword ASR',
  modelRetry: 'Retry',
  sparklineAria: '30-day media trend',
  tourInteractive: 'Interactive tour',
  tourManual: 'Manual',
  tourWelcomeTitle: 'Welcome to OneAsr',
  tourWelcomeBody: 'Walk the real controls for a minute or two, or skip — reopen anytime from Guide in the toolbar.',
  tourStart: 'Start tour',
  tourSkip: 'Skip',
  tourNext: 'Next',
  tourFinish: 'Done',
  tourClickHint: 'Click the highlighted control to continue',
  tourClickOrNextHint: 'Click the highlighted control, or Skip step',
  tourSkipStep: 'Skip step',
  tourSkipWait: 'Skip download wait',
  tourStatusTitle: 'Model status',
  tourStatusBody: 'The status bar shows whether models are ready. If not, click it for Settings → Models — or Skip step.',
  tourCliTitle: 'Engine CLI outdated',
  tourCliBody: 'The orange status warning means the engine CLI needs an update — not that models are missing. Click Settings to open Settings → Models, or Skip step.',
  tourModelsTitle: 'Models',
  tourModelsBody: 'Use the left nav for Models / Hotwords and more. Pick ASR size, aligner, and weight folders; the status bar reflects this set.',
  tourDownloadTitle: 'Download weights',
  tourDownloadBody: 'When weights are missing, use this download control. Downloads do not stop the tour; skip and fetch later if you like.',
  tourHotwordsTabTitle: 'Hotwords tab',
  tourHotwordsTabBody: 'Open Hotwords to manage glossaries for names and acronyms.',
  tourHotwordsTitle: 'Hotword groups',
  tourHotwordsBody: 'Organize terms by group. Rows can pick a group; an empty default group is not bound to new tasks.',
  tourOutputTabTitle: 'Output tab',
  tourOutputTabBody: 'Open Output for subtitle formats and save location.',
  tourOutputTitle: 'Output settings',
  tourOutputBody: 'Keep at least one of SRT / TXT / ASS. Aligning a transcript needs a timed format (SRT or ASS).',
  tourSettingsCloseTitle: 'Close settings',
  tourSettingsCloseBody: 'Save when you change things. Click Close (or Skip step) to return to the main screen.',
  tourAddTitle: 'Add files',
  tourAddBody: 'Click Add or drop media into the window. A matching txt / srt attaches as a transcript. Use Next to skip adding for now.',
  tourDropzoneTitle: 'Drop zone',
  tourDropzoneBody: 'When the list is empty and models are ready, drop media here.',
  tourEmptySettingsTitle: 'Set up models first',
  tourEmptySettingsBody: 'The canvas cannot accept drops until models are ready. Click Open settings to prepare weights, or Skip step.',
  tourTaskLangTitle: 'Recognition language',
  tourTaskLangBody: 'Language for this row only. You can apply it to every row.',
  tourTaskHotwordsTitle: 'Row hotwords',
  tourTaskHotwordsBody: 'Pick a glossary for this row. Languages other than Chinese and English do not support hotwords.',
  tourTaskSepTitle: 'Vocal separation',
  tourTaskSepBody: 'Turn on for noisy audio. Needs the separation model; hollow is off, solid is on.',
  tourTaskTranscriptTitle: 'Transcript',
  tourTaskTranscriptBody: 'Accent when attached. You can also drop a txt / srt on the row. Align with Start on the right.',
  tourTaskStartTitle: 'Start',
  tourTaskStartBody: 'Starts this row. Start all runs pending and failed rows only. The tour will not press it for you.',
  tourTaskOutputTitle: 'Open output',
  tourTaskOutputBody: 'When finished, open the folder with the subtitles.',
  tourStatsTitle: 'Stats panel',
  tourStatsBody: 'This is the Stats panel: throughput and recent activity. Click Next when you are done looking.',
  tourGuideReopenTitle: 'Open again',
  tourGuideReopenBody: 'Use Guide anytime for the interactive tour or the static manual.',
  tourDoneTitle: 'You are set',
  tourDoneBody: 'Tour finished. Esc also exits the tour anytime and never cancels a running task.',
}

export function resolveUiLang(setting: string): UiLang {
  if (setting === 'zh' || setting === 'en') return setting
  const sys = (navigator.language || 'en').toLowerCase()
  return sys.startsWith('zh') ? 'zh' : 'en'
}

export function t(lang: UiLang): Dict {
  return lang === 'zh' ? zh : en
}

export const LANG_OPTIONS = [
  { id: 'zh', label: '中文' },
  { id: 'en', label: 'English' },
  { id: 'yue', label: '粤语' },
  { id: 'ja', label: '日本語' },
  { id: 'ko', label: '한국어' },
  { id: 'auto', label: 'Auto' }
]

/**
 * 阶段名：按 CLI 事件里的稳定 `key` 出文案（label 跟 CLI 进程语言走，可能是系统语言，
 * 跟界面语言不一致）。认不出的 key 就退回 CLI 给的 label——不猜。
 */
export function stageLabel(lang: UiLang, key: string | undefined, fallback?: string): string {
  const s = t(lang) as unknown as Record<string, string>
  if (!key) return fallback ?? ''
  const camel = key.replace(/(^|_)([a-z])/g, (_m, _p, c: string) => c.toUpperCase())
  const value = s[`stage${camel}`]
  return value ?? fallback ?? key
}

/**
 * 阶段名后面接分段计数（转写 2/5 / Transcribing 2/5）。
 * 没有分段、或只有一段时保持原名，不出现 1/1。名字为空就不拼，交给「进行中」兜底。
 */
export function stageWithChunk(
  name: string,
  chunk: number | undefined,
  chunkTotal: number | undefined
): string {
  if (
    name &&
    typeof chunk === 'number' &&
    typeof chunkTotal === 'number' &&
    Number.isInteger(chunk) &&
    Number.isInteger(chunkTotal) &&
    chunk >= 0 &&
    chunkTotal > 1
  ) {
    return `${name} ${chunk}/${chunkTotal}`
  }
  return name
}

/** 「已忽略 N 个不支持的文件」/「N unsupported file(s) ignored」：拖放过滤的说明。 */
export function unsupportedCount(lang: UiLang, n: number): string {
  return lang === 'zh'
    ? `已忽略 ${n} 个不支持的文件（支持 wav / mp3 / mp4 / mkv …）`
    : `${n} unsupported file${n > 1 ? 's' : ''} ignored (wav / mp3 / mp4 / mkv …)`
}

/** 任务状态 → 界面文案（统计页的最近会话直接用）。 */
export function taskStatusLabel(lang: UiLang, status: string): string {
  const s = t(lang) as unknown as Record<string, string>
  return s[status] ?? status
}

/** 热力图格子的悬停说明：日期 + 时长 + 成功/失败数。 */
export function heatTooltip(
  lang: UiLang,
  day: string,
  mediaSec: number,
  tasks: number,
  errors: number
): string {
  const s = t(lang)
  return `${day}: ${formatDuration(mediaSec)} · ${tasks} ${s.success} · ${errors} ${s.failures}`
}

/** 「排队第 N 位」/「Queued · #N」：队列位次（Dict 只放字符串，带参数的走 helper）。 */
export function queueRank(lang: UiLang, n: number): string {
  return lang === 'zh' ? `排队第 ${n} 位` : `Queued · #${n}`
}

/**
 * 实时倍速（RTFx）：音视频时长 ÷ 本次处理耗时。
 *
 * 跟 master 的耗时弹卡同一口径（`e56299b`）：这是**端到端**的倍速，
 * 跟旁边的总耗时互为倒数——模型加载那几段也都在分母里。
 * 算不出来就不显示，绝不显示 ∞ / NaN：时长未知、耗时为零、非法数值一律 None。
 * 过了 100× 就把小数去掉，免得撑宽弹卡。
 */
export function realtimeFactorLabel(
  lang: UiLang,
  mediaSec: number | undefined | null,
  processMs: number | undefined | null
): string | null {
  if (mediaSec == null || !Number.isFinite(mediaSec) || mediaSec <= 0) return null
  if (processMs == null || !Number.isFinite(processMs) || processMs <= 0) return null
  const factor = mediaSec / (processMs / 1000)
  if (!Number.isFinite(factor)) return null
  const number = factor >= 100 ? factor.toFixed(0) : factor.toFixed(1)
  return lang === 'zh' ? `${number} 倍速` : `${number}×`
}

/** 「还有 N 条」/「N more」：折叠列表的计数后缀。Dict 只放字符串，带参数的走 helper。 */
export function moreItems(lang: UiLang, n: number): string {
  return lang === 'zh' ? `还有 ${n} 条` : `${n} more`
}

/** 文稿 chip / 摘要里的「47 行 · 3.2k 字」。 */
export function transcriptSummary(lang: UiLang, lines: number, chars: number): string {
  const c = chars >= 1000 ? `${(chars / 1000).toFixed(1)}k` : String(chars)
  if (lang === 'zh') return `${lines} 行 · ${c} 字`
  return `${lines} ${lines === 1 ? 'line' : 'lines'} · ${c} chars`
}

/** 语速提示：中文按字、英文按词，两条带子由 core 的 plausible() 决定。 */
export function transcriptRate(
  lang: UiLang,
  rate: { perSecond: number; spaced: boolean; plausible: boolean }
): string {
  const value = rate.perSecond.toFixed(1)
  if (lang === 'zh') {
    return `${value} ${rate.spaced ? '词/秒' : '字/秒'} · ${
      rate.plausible ? '语速正常' : '与音频长度不匹配'
    }`
  }
  return `${value} ${rate.spaced ? 'words/s' : 'chars/s'} · ${
    rate.plausible ? 'looks right' : 'mismatch with media length'
  }`
}

/** 没配上媒体的文稿点名报出来，不静默吞掉。 */
export function transcriptsUnpaired(lang: UiLang, names: string[]): string {
  const head = names.slice(0, 3).join(', ')
  const more =
    names.length > 3
      ? lang === 'zh'
        ? ` 等 ${names.length} 个`
        : ` and ${names.length - 3} more`
      : ''
  return lang === 'zh'
    ? `没有同名音视频的文稿：${head}${more}`
    : `Transcripts without matching media: ${head}${more}`
}

/** 配对成功的轻提示。 */
export function transcriptsPaired(lang: UiLang, n: number): string {
  return lang === 'zh'
    ? `已配上 ${n} 份文稿`
    : `${n} transcript${n === 1 ? '' : 's'} paired`
}

export function confirmClearTasks(lang: UiLang, n: number): string {
  if (lang === 'zh') return `清空这 ${n} 条任务？\n正在运行的会留下。`
  return n === 1
    ? 'Clear this task?\nA task that is running now stays.'
    : `Clear these ${n} tasks?\nA task that is running now stays.`
}

export function formatDuration(sec: number | null | undefined): string {
  if (sec == null || !Number.isFinite(sec)) return '—'
  if (sec < 60) return `${sec.toFixed(1)}s`
  const m = Math.floor(sec / 60)
  const s = Math.round(sec % 60)
  if (m < 60) return `${m}m ${s}s`
  const h = Math.floor(m / 60)
  return `${h}h ${m % 60}m`
}
