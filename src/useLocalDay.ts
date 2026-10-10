import { useEffect, useState } from 'react'
export function useLocalDay() {
  const [now, setNow] = useState(() => new Date())
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout>
    function refresh() {
      const date = new Date()
      setNow(date)
      clearTimeout(timer)
      const tomorrow = new Date(date.getFullYear(), date.getMonth(), date.getDate() + 1)
      timer = setTimeout(refresh, tomorrow.getTime() - date.getTime() + 100)
    }
    function onVisible() { if (!document.hidden) refresh() }
    const date = new Date()
    timer = setTimeout(refresh, new Date(date.getFullYear(), date.getMonth(), date.getDate() + 1).getTime() - date.getTime() + 100)
    document.addEventListener('visibilitychange', onVisible)
    window.addEventListener('focus', refresh)
    return () => { clearTimeout(timer); document.removeEventListener('visibilitychange', onVisible); window.removeEventListener('focus', refresh) }
  }, [])
  return now
}
