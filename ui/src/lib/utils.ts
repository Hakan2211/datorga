import { clsx, type ClassValue } from 'clsx'
import { twMerge } from 'tailwind-merge'

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

/** A choice kept in localStorage (view toggles, sort order), or `fallback` when unset or unknown. */
export function storedChoice<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  const v = localStorage.getItem(key)
  return allowed.includes(v as T) ? (v as T) : fallback
}
