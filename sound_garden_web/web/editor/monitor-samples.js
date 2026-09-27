// Preserve every worklet frame: the spectrum needs consecutive samples.
export function stereoFrames(samples) {
  const stereo = [];
  for (let i = 0; i + 1 < samples.length; i += 2) {
    stereo.push([samples[i], samples[i + 1]]);
  }
  return stereo;
}
