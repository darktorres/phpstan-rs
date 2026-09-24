<?php
            function f(): string {
                $capture = new stdClass();
                $capture->lastMessage = 'hi';
                return $capture->lastMessage;
            }
        