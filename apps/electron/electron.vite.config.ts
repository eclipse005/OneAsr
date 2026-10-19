import { resolve } from 'path'
import { defineConfig, externalizeDepsPlugin } from 'electron-vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  main: {
    plugins: [externalizeDepsPlugin()],
    build: {
      rollupOptions: {
        input: {
          index: resolve(__dirname, 'src/main/index.ts')
        }
      }
    }
  },
  preload: {
    plugins: [externalizeDepsPlugin()],
    build: {
      rollupOptions: {
        input: {
          index: resolve(__dirname, 'src/preload/index.ts')
        }
      }
    }
  },
  renderer: {
    root: resolve(__dirname, 'src/renderer'),
    // 图标只有一份真源：仓库根的 assets/（Electron 与旧版共享的图标源）。
    // CSS 里用 url('@assets/icons/xxx.svg')，Vite 会当资源打包并改成 file:// 下可用的相对路径。
    resolve: {
      alias: {
        '@assets': resolve(__dirname, '../../assets')
      }
    },
    // 开发服务器默认只放行 apps/electron。图标在仓库根的 assets/，
    // 不放行的话 mask 读不到 SVG，行内按钮会变成空的。
    server: {
      fs: {
        allow: [resolve(__dirname), resolve(__dirname, '../../assets')]
      }
    },
    build: {
      rollupOptions: {
        input: {
          index: resolve(__dirname, 'src/renderer/index.html')
        }
      }
    },
    plugins: [react()]
  }
})
