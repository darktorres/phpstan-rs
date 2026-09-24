<?php
            class UserMailNotification {}
            /**
             * @param list<UserMailNotification> $dataUsers
             * @return ($isActionSend is true ? list<string> : list<UserMailNotification>)
             */
            function f(array $dataUsers, bool $isActionSend): array {
                $returnList = [];
                foreach ($dataUsers as $nbmUser) {
                    if ($isActionSend) {
                        $returnList[] = 'checkKey';
                    } else {
                        $returnList[] = $nbmUser;
                    }
                }
                return $returnList;
            }
        