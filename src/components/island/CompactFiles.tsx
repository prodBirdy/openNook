import { CompactWrapper } from './CompactWrapper';
import { IconFiles } from '@tabler/icons-react';
import { FileItem } from '../FileTray';

interface CompactFilesProps {
    files: FileItem[];
    isHovered: boolean;
    baseNotchWidth: number;
    contentOpacity: number;
}

export function CompactFiles({
    files,
    isHovered,
    baseNotchWidth,
    contentOpacity
}: CompactFilesProps) {
    return (
        <CompactWrapper
            id="files-content"
            className="island-content files-content"
            baseNotchWidth={baseNotchWidth}
            isHovered={isHovered}
            contentOpacity={contentOpacity}
            left={
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <IconFiles size={20} color="white" stroke={1.5} />
                </div>
            }
            right={
                <div style={{ display: 'flex', alignItems: 'center', color: 'white' }}>
                    {files.length}
                </div>
            }
        />
    );
}
