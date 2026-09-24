<?php
            interface A {} interface B {}
            function a(?null $x, A&int $y, A&B&A $z): (A&B)|(A&string) { return $z; }
            