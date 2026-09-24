<?php
            interface ImageInterface1 {}
            class ImageGd1 implements ImageInterface1 {
                public function destroy(): bool { return true; }
            }
            function f(ImageInterface1 $image): void {
                if (method_exists($image, 'destroy')) {
                    $image->destroy();
                }
            }
        