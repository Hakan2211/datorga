import { useQuery } from '@tanstack/react-query'
import { q } from '@/lib/api'
import { initials } from '@/lib/patients'
import { cn } from '@/lib/utils'

/**
 * Round portrait, or the initials on a warm tint when there's no photo. The photo is fetched only when `hasPhoto`;
 * `src` (e.g. an unsaved preview) wins over the stored one.
 */
export function PatientAvatar({
  patientId,
  hasPhoto,
  src: override,
  firstName,
  lastName,
  className,
}: {
  patientId: string
  hasPhoto: boolean
  src?: string
  firstName: string
  lastName: string
  className?: string
}) {
  const photo = useQuery({ ...q.patientPhoto(patientId), enabled: hasPhoto && !override })
  const src = override ?? (hasPhoto ? photo.data : null)
  return (
    <div
      className={cn(
        'flex size-16 shrink-0 items-center justify-center overflow-hidden rounded-full bg-accent font-serif text-xl text-accent-foreground select-none',
        className,
      )}
      aria-hidden
    >
      {src ? <img src={src} alt="" className="size-full object-cover" draggable={false} /> : initials(firstName, lastName)}
    </div>
  )
}
