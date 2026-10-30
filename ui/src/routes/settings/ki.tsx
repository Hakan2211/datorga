import { createFileRoute } from '@tanstack/react-router'
import { CostsCard } from '@/components/costs-card'
import { LlmCard } from '@/components/llm-settings'

export const Route = createFileRoute('/settings/ki')({ component: Ai })

function Ai() {
  return (
    <>
      <LlmCard />
      <div id="costs" className="scroll-mt-4">
        <CostsCard />
      </div>
    </>
  )
}
