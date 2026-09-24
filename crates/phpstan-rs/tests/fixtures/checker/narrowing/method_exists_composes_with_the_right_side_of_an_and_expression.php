<?php
            interface ImageInterface4 {}
            function f(ImageInterface4 $image): void {
                if (method_exists($image, 'destroy') && $image->destroy()) {
                }
            }
        