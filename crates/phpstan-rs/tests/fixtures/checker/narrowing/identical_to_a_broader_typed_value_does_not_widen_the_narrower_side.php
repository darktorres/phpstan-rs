<?php
            class ImageId {
                public static function from(int $value): self { return new self(); }
            }
            function getRefererImageId(): ?int { return null; }
            function f(int $image_id): void {
                if (getRefererImageId() === $image_id) {
                    // no-op - neither branch reassigns $image_id
                }
                ImageId::from($image_id);
            }
        