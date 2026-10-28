import { useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { api, q } from '@/lib/api'

/** The remembered mic; a device that disappeared falls back to the system default. */
export function useCaptureDevice() {
  const qc = useQueryClient()
  const devices = useQuery(q.audioDevices())
  const saved = useQuery(q.setting<string>('audio_device'))
  const [picked, setPicked] = useState<string | null>(null)
  const preferred = picked ?? saved.data ?? 'default'
  const deviceId = devices.data?.some((d) => d.id === preferred) ? preferred : 'default'
  const setDeviceId = (id: string) => {
    setPicked(id)
    void api.setSetting('audio_device', id).then(() => qc.invalidateQueries({ queryKey: ['setting', 'audio_device'] }))
  }
  return { devices: devices.data ?? [], deviceId, setDeviceId, startId: deviceId === 'default' ? null : deviceId }
}

export function formatDuration(ms: number): string {
  const s = Math.floor(ms / 1000)
  const h = Math.floor(s / 3600)
  const mm = String(Math.floor((s % 3600) / 60)).padStart(2, '0')
  const ss = String(s % 60).padStart(2, '0')
  return h ? `${h}:${mm}:${ss}` : `${mm}:${ss}`
}
