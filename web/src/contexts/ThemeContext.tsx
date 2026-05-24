import { createContext, useContext, useState, useEffect, type ReactNode } from 'react'

export type Theme = 'terminal' | 'dashboard'

interface Ctx { theme: Theme; toggle: () => void }
const ThemeContext = createContext<Ctx>({ theme: 'terminal', toggle: () => {} })

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setTheme] = useState<Theme>(
    () => (localStorage.getItem('pd-theme') as Theme) || 'terminal'
  )

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme)
  }, [theme])

  const toggle = () =>
    setTheme(t => {
      const next = t === 'terminal' ? 'dashboard' : 'terminal'
      localStorage.setItem('pd-theme', next)
      return next
    })

  return <ThemeContext.Provider value={{ theme, toggle }}>{children}</ThemeContext.Provider>
}

export const useTheme = () => useContext(ThemeContext)
