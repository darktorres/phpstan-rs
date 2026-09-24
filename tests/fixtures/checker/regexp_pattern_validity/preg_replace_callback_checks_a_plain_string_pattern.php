<?php
            preg_replace_callback('/foo/y', fn ($m) => $m[0], 'x');
        