import { Home, Settings, History } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useRoute, type Route } from '../../lib/router'
import { AccessibilityBanner } from './AccessibilityBanner'
import { BrandMark } from '../BrandMark'
const navItems: { id: Route; labelKey: string; icon: typeof Home }[] = [
  { id: 'home', labelKey: 'nav.home', icon: Home },
  { id: 'settings', labelKey: 'nav.settings', icon: Settings },
  { id: 'history', labelKey: 'nav.history', icon: History },
]
export function MainLayout({ children }: { children: React.ReactNode }) {
  const { route, navigate } = useRoute()
  const { t } = useTranslation()
  return (
    <div className="w-full h-full flex bg-bg-primary text-text-primary">
      <aside className="w-[180px] flex flex-col border-r border-border jelly-surface-flat shrink-0">
        <div className="px-5 pt-6 pb-5 flex items-center gap-2.5" data-tauri-drag-region>
          <BrandMark size={30} className="shrink-0 pointer-events-none" />
          <div className="min-w-0 pointer-events-none">
            <h1 className="text-[15px] font-semibold leading-tight">Voice Input MVP</h1>
            <p className="text-[12px] text-text-tertiary mt-0.5">中文语音输入</p>
          </div>
        </div>
        <nav className="flex-1 px-3 space-y-1" aria-label={t('nav.mainNavigation')}>
          {navItems.map(({ id, labelKey, icon: Icon }) => (
            <button
              key={id}
              onClick={() => navigate(id)}
              aria-current={route === id ? 'page' : undefined}
              className={`flex items-center gap-2.5 w-full px-3 py-2 rounded-[8px] text-[13px] text-left ${route === id ? 'jelly-nav-active font-medium' : 'hover:bg-bg-secondary'}`}
            >
              <Icon size={16} />
              {t(labelKey)}
            </button>
          ))}
        </nav>
        <p className="px-5 pb-5 text-[11px] text-text-tertiary">个人版 · 历史仅存本机</p>
      </aside>
      <main className="flex-1 min-w-0 flex flex-col">
        <AccessibilityBanner />
        <div className="flex-1 overflow-y-auto">{children}</div>
      </main>
    </div>
  )
}
