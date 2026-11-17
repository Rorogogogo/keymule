import { Hero, Nav } from './components/Hero'
import { Agents, Features, Film, How, Pains } from './components/Sections'
import { Faq, Footer, Install, Security } from './components/More'

export default function App() {
  return (
    <div className="grain relative">
      <a href="#main" className="sr-only focus:not-sr-only focus:fixed focus:top-3 focus:left-3 focus:z-50 focus:rounded-full focus:bg-ink focus:px-4 focus:py-2 focus:text-paper">
        Skip to content
      </a>
      <Nav />
      <main id="main" className="relative">
        <Hero />
        <Agents />
        <Pains />
        <Film />
        <How />
        <Features />
        <Security />
        <Install />
        <Faq />
      </main>
      <Footer />
    </div>
  )
}
