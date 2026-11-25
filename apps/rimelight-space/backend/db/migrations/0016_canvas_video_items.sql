-- Video items on canvases (CV-V.4, decision 0046): an item of kind 'Video' shows a video asset (props: asset, volume,
-- muted, loop, plus the shared transform props). Only the allowed kinds change.
ALTER TABLE canvas_items DROP CONSTRAINT canvas_items_kind_check;
ALTER TABLE canvas_items ADD CONSTRAINT canvas_items_kind_check
    CHECK (kind IN ('Image', 'Text', 'Shape', 'Stroke', 'Frame', 'Video'));
