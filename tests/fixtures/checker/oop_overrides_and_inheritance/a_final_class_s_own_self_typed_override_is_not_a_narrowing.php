<?php
            interface ImageInterface {
                public function compose(ImageBackend $overlay): bool;
            }
            final class ImageBackend implements ImageInterface {
                public function compose(self $overlay): bool {
                    return true;
                }
            }
        