<?php
            interface ImageInterface2 {}
            function f(ImageInterface2 $image): void {
                if (method_exists($image, 'destroy')) {
                    $image->somethingElse();
                }
            }
        