<?php
            class GdImage {}
            /** @return resource|GdImage|false */
            function imagecreatefromstring(string $data): GdImage|false {}
            class ImageGd {
                private GdImage $image;
                public function __construct(string $data) {
                    $img = imagecreatefromstring($data);
                    if ($img === false) {
                        throw new Exception('failed');
                    }
                    $this->image = $img;
                }
                public function get(): GdImage {
                    return $this->image;
                }
            }
        