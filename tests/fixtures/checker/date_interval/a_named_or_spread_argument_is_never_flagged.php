<?php
            new DateInterval(duration: 'garbage');
            $args = ['garbage'];
            new DateInterval(...$args);
        