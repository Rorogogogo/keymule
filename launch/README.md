# keymule launch film

The 30-second launch film in the main README, made with [Remotion](https://www.remotion.dev)
and GSAP. English and Chinese versions.

```sh
cd launch
npm i --loglevel=error
python3 -I scripts/make_audio.py      # the score and sound effects -> public/audio/*.wav
npm run dev                           # preview in Remotion Studio
npx remotion render KeymuleLaunch out/keymule-launch.mp4
npx remotion render KeymuleLaunchZh out/keymule-launch-zh.mp4
```

- `src/Launch.tsx`: the film. `src/gsapRemotion.ts` seeks GSAP timelines frame by frame.
- `src/Proto.tsx`: the `KeyJourneyProto` motion prototype.
- `public/shots/`: real keymule UI, captured from a debug build with demo data.
- `scripts/make_audio.py`: an original score and effects, synthesised in code (needs numpy).
  The `.wav` files it writes aren't committed; neither are the renders in `out/`.
- `social/`: covers and thumbnails (HTML source and PNG).
- `old/`: earlier versions of the film, for reference.
