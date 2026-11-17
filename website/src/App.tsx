import { Hero, Nav } from './components/Hero'
import { Agents, Features, Film, How, Pains } from './components/Sections'
import { Faq, Footer, Install, Security } from './components/More'

export default function App() {
  return (
    <div className="relative overflow-x-clip">
      <a href="#main" className="sr-only focus:not-sr-only focus:fixed focus:top-3 focus:left-3 focus:z-50 focus:rounded-full focus:bg-white focus:px-4 focus:py-2 focus:text-black">
        Skip to content
      </a>
      <Nav />
      <main id="main">
        <Hero />
        <Agents />
        <How />
        <Pains />
        <Features />
        <Film />
        <Security />
        <Install />
        <Faq />
      </main>
      <Footer />
    </div>
  )
}
