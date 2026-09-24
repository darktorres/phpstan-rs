<?php
            preg_replace_callback_array(['/foo/y' => fn ($m) => $m[0]], 'x');
        