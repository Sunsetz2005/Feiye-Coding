import { describe, expect, it } from "vitest";
import { isLocalFsPath, isViewableVideoSrc } from "./VideoUi";

describe("VideoUi ResourceHandle sources", () => {
  it("never treats URL/protocol sources as local filesystem paths", () => {
    for (const value of [
      undefined,
      "https://example.test/video.mp4",
      "data:video/mp4;base64,AA==",
      "blob:video",
      "asset://localhost/video.mp4",
      "https://asset.localhost/video.mp4",
      "media://localhost/video.mp4",
      "https://media.localhost/video.mp4",
      "resource://localhost/opaque-handle",
      "https://resource.localhost/opaque-handle",
      "relative/video.mp4",
    ]) {
      expect(isLocalFsPath(value)).toBe(false);
    }
    expect(isLocalFsPath("/trusted/video.mp4")).toBe(true);
    expect(isLocalFsPath("C:\\trusted\\video.mp4")).toBe(true);
  });

  it("accepts every supported viewable scheme, including resource handles", () => {
    for (const value of [
      "http://example.test/video.mp4",
      "https://example.test/video.mp4",
      "data:video/mp4;base64,AA==",
      "blob:video",
      "asset://localhost/video.mp4",
      "media://localhost/video.mp4",
      "resource://localhost/opaque-handle",
      "https://asset.localhost/video.mp4",
      "https://media.localhost/video.mp4",
      "https://resource.localhost/opaque-handle",
    ]) {
      expect(isViewableVideoSrc(value)).toBe(true);
    }
    expect(isViewableVideoSrc("/trusted/video.mp4")).toBe(false);
  });
});
