<?php
            interface ImageInterface5 {}
            function f(ImageInterface5 $image, bool $flag): void {
                if ($flag) {
                    if (method_exists($image, 'destroy')) {
                    }
                } else {
                }
                $image->destroy();
            }
        