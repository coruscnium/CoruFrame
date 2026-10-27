import deimos from "../assets/wf/DeimosPlanet.png";
import earth from "../assets/wf/EarthPlanet.png";
import venus from "../assets/wf/VenusPlanet.png";

// Star-chart planet art for the world cycles. Cetus and the Plains share
// Earth, Orb Vallis is on Venus and the Cambion Drift on Deimos.
export const cycleImages: Record<string, string> = {
  earth,
  cetus: earth,
  vallis: venus,
  cambion: deimos,
};
