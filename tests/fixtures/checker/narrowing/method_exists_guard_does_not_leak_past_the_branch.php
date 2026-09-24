<?php
            interface ImageInterface3 {}
            function f(ImageInterface3 $image): void {
                if (method_exists($image, 'destroy')) {
                    $image->destroy();
                }
                $image->destroy();
            }
        