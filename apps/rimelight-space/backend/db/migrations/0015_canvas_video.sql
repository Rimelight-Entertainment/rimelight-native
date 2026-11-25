-- Video on canvases (CV-V.1, decision 0046). A canvas asset is an image or a video. Videos are MP4, MOV, WebM or
-- Matroska, at most 500 MB (images stay at most 50 MB); the client reads their duration and whether they have sound.
-- Video thumbnails use the same 256 and 1024 px WebP keys as images (thumb_keys), made by the client from one frame.
ALTER TABLE canvas_assets ADD COLUMN media text NOT NULL DEFAULT 'Image' CHECK (media IN ('Image', 'Video'));
ALTER TABLE canvas_assets ADD COLUMN duration_ms bigint CHECK (duration_ms IS NULL OR duration_ms > 0);
ALTER TABLE canvas_assets ADD COLUMN has_audio boolean NOT NULL DEFAULT false;

ALTER TABLE canvas_assets DROP CONSTRAINT canvas_assets_mime_check;
ALTER TABLE canvas_assets ADD CONSTRAINT canvas_assets_mime_check CHECK (
    (media = 'Image' AND mime IN ('image/png', 'image/jpeg', 'image/webp', 'image/gif'))
    OR (media = 'Video' AND mime IN ('video/mp4', 'video/quicktime', 'video/webm', 'video/x-matroska'))
);

ALTER TABLE canvas_assets DROP CONSTRAINT canvas_assets_bytes_check;
ALTER TABLE canvas_assets ADD CONSTRAINT canvas_assets_bytes_check CHECK (
    bytes > 0 AND bytes <= CASE WHEN media = 'Video' THEN 524288000 ELSE 52428800 END   -- 500 MB video, 50 MB image
);

-- A video has a duration; an image has none.
ALTER TABLE canvas_assets ADD CONSTRAINT canvas_assets_duration_check CHECK ((media = 'Video') = (duration_ms IS NOT NULL));
