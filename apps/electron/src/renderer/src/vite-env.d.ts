/// <reference types="vite/client" />

interface Window {
  oneasr: import('../../preload/api').OneAsrApi
}

declare module '*.wav?url' {
  const src: string
  export default src
}

declare module '@assets/sounds/*.wav?url' {
  const src: string
  export default src
}
