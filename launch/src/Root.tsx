import { Composition } from "remotion";
import { Launch } from "./Launch";
import { Proto } from "./Proto";

export const RemotionRoot: React.FC = () => {
  return (
    <>
    <Composition
      id="KeyJourneyProto"
      component={Proto}
      durationInFrames={270}
      fps={30}
      width={1920}
      height={1080}
    />
    <Composition
      id="KeymuleLaunch"
      component={Launch}
      durationInFrames={900}
      fps={30}
      width={1920}
      height={1080}
      defaultProps={{ lang: "en" as const }}
    />
    <Composition
      id="KeymuleLaunchZh"
      component={Launch}
      durationInFrames={900}
      fps={30}
      width={1920}
      height={1080}
      defaultProps={{ lang: "zh" as const }}
    />
    </>
  );
};
